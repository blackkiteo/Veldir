use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn list(path: &Path) -> Result<Vec<Entry>, String> {
    let it =
        fs::read_dir(path).map_err(|e| format!("Impossibile leggere {}: {e}", path.display()))?;
    let mut result = Vec::new();
    for item in it {
        let item = match item {
            Ok(v) => v,
            Err(_) => continue,
        };
        let metadata = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        result.push(Entry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: item.path(),
            is_dir: metadata.is_dir(),
            size: if metadata.is_file() {
                metadata.len()
            } else {
                0
            },
            modified: metadata.modified().ok(),
        });
    }
    result.sort_unstable_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(result)
}

#[derive(Debug, Clone)]
pub enum Job {
    Transfer {
        sources: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
        policy: crate::transfer::ConflictPolicy,
    },
    NewFolder {
        parent: PathBuf,
        name: String,
    },
    Rename {
        source: PathBuf,
        name: String,
    },
    Copy {
        source: PathBuf,
        destination_folder: PathBuf,
    },
    Move {
        source: PathBuf,
        destination_folder: PathBuf,
    },
    Trash {
        source: PathBuf,
    },
    CopyMany {
        sources: Vec<PathBuf>,
        destination_folder: PathBuf,
    },
    MoveMany {
        sources: Vec<PathBuf>,
        destination_folder: PathBuf,
    },
    TrashMany {
        sources: Vec<PathBuf>,
    },
    RenameMany {
        changes: Vec<(PathBuf, String)>,
    },
}

pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." || name.trim() != name {
        return Err("Inserisci un nome valido, senza spazi iniziali/finali".into());
    }
    if name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("Il nome contiene caratteri non consentiti su Windows".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.as_bytes()[3].is_ascii_digit()
        && stem.as_bytes()[3] != b'0')
    {
        return Err("Nome riservato di Windows".into());
    }
    Ok(())
}

pub(crate) fn validate_copy_destination(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    check_cancelled(cancelled)?;
    let meta =
        fs::symlink_metadata(source).map_err(|e| format!("Sorgente {}: {e}", source.display()))?;
    if meta.file_type().is_symlink() || is_windows_reparse_point(&meta) {
        return Err(format!(
            "Collegamento o junction non supportati: {}",
            source.display()
        ));
    }
    if !meta.is_file() && !meta.is_dir() {
        return Err("Tipo di file non supportato".into());
    }
    if meta.is_dir() {
        check_cancelled(cancelled)?;
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        check_cancelled(cancelled)?;
        let destination = destination.canonicalize().map_err(|e| e.to_string())?;
        if destination.starts_with(source) {
            return Err("Non è possibile copiare una cartella al suo interno".into());
        }
    }
    Ok(())
}

fn target_path(source: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    if !dest_dir.is_dir() {
        return Err("La destinazione non è una cartella accessibile".into());
    }
    let name = source
        .file_name()
        .ok_or("Impossibile identificare il nome sorgente")?;
    let target = dest_dir.join(name);
    if path_exists(&target)? {
        return Err(format!(
            "Esiste già un elemento con il nome {}: nessuna sovrascrittura eseguita",
            target.display()
        ));
    }
    if source.is_dir() {
        let src = source.canonicalize().map_err(|e| e.to_string())?;
        let dest = dest_dir.canonicalize().map_err(|e| e.to_string())?;
        if dest.starts_with(&src) {
            return Err("Non è possibile copiare una cartella al suo interno".into());
        }
    }
    Ok(target)
}

#[cfg(windows)]
fn is_windows_reparse_point(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_REPARSE_POINT: also detects directory junctions.
    meta.file_attributes() & 0x0000_0400 != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_meta: &fs::Metadata) -> bool {
    false
}

/// Copy to an isolated staging directory, NEVER directly to the destination.
/// On failure tempfile removes only staging files created by Veldir.
/// Junctions and symlinks are rejected to avoid unsafe recursive traversal.
fn copy_no_links(source: &Path, target: &Path) -> Result<(), String> {
    let cancelled = AtomicBool::new(false);
    copy_tree_progress(source, target, &cancelled, &mut |_| {})
}

/// One budget is shared by every source in a copy batch. Entries are charged
/// before enqueue (including roots); the pending path queue has its own bound.
struct ScanBudget {
    entries: usize,
    pending_bytes: usize,
    max_entries: usize,
    max_pending_bytes: usize,
}
impl Default for ScanBudget {
    fn default() -> Self {
        Self {
            entries: 0,
            pending_bytes: 0,
            max_entries: 100_000,
            max_pending_bytes: 32 * 1024 * 1024,
        }
    }
}
impl ScanBudget {
    fn path_bytes(path: &Path) -> Result<usize, String> {
        path.as_os_str()
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(std::mem::size_of::<PathBuf>()))
            .ok_or_else(|| "Percorso troppo grande per il preflight".into())
    }
    fn enqueue(&mut self, path: &Path) -> Result<usize, String> {
        let entries = self
            .entries
            .checked_add(1)
            .ok_or("Troppi elementi nel preflight")?;
        if entries > self.max_entries {
            return Err(format!(
                "Preflight interrotto: massimo {} elementi per batch",
                self.max_entries
            ));
        }
        let bytes = Self::path_bytes(path)?;
        let pending_bytes = self
            .pending_bytes
            .checked_add(bytes)
            .ok_or("Memoria preflight esaurita")?;
        if pending_bytes > self.max_pending_bytes {
            return Err("Preflight interrotto: limite memoria percorsi pendenti".into());
        }
        self.entries = entries;
        self.pending_bytes = pending_bytes;
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanOperation {
    Metadata,
    ReadDir,
    Next,
}

#[cfg(test)]
struct ScanTestSettings {
    max_entries: usize,
    max_pending_bytes: usize,
    before_io: Box<dyn FnMut(ScanOperation, &Path) -> Result<(), String>>,
}
#[cfg(test)]
thread_local! {
    static SCAN_TEST_SETTINGS: std::cell::RefCell<Option<ScanTestSettings>> = const { std::cell::RefCell::new(None) };
}
/// Thread-local fault injection keeps parallel tests independent; never compiled in release.
#[cfg(test)]
pub(crate) fn with_scan_test_settings<T>(
    max_entries: usize,
    max_pending_bytes: usize,
    before_io: impl FnMut(ScanOperation, &Path) -> Result<(), String> + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<ScanTestSettings>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCAN_TEST_SETTINGS.with(|settings| {
                settings.replace(self.0.take());
            });
        }
    }
    let previous = SCAN_TEST_SETTINGS.with(|settings| {
        settings.replace(Some(ScanTestSettings {
            max_entries,
            max_pending_bytes,
            before_io: Box::new(before_io),
        }))
    });
    let _restore = Restore(previous);
    action()
}

pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Preflight annullato; nessun nuovo elemento trasferito".into())
    } else {
        Ok(())
    }
}

/// Only a genuine NotFound means absence; network and permission errors fail closed.
pub(crate) fn path_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Impossibile verificare {}: {error}",
            path.display()
        )),
    }
}

fn scan_tree(
    source: &Path,
    cancelled: &AtomicBool,
    budget: &mut ScanBudget,
    before_io: &mut impl FnMut(ScanOperation, &Path) -> Result<(), String>,
) -> Result<u64, String> {
    check_cancelled(cancelled)?;
    let bytes = budget.enqueue(source)?;
    let mut stack = vec![(source.to_path_buf(), bytes)];
    let mut total = 0u64;
    let result = (|| {
        while let Some((path, bytes)) = stack.pop() {
            budget.pending_bytes -= bytes;
            check_cancelled(cancelled)?;
            before_io(ScanOperation::Metadata, &path)?;
            check_cancelled(cancelled)?;
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("Preflight {}: {e}", path.display()))?;
            if meta.file_type().is_symlink() || is_windows_reparse_point(&meta) {
                return Err(format!(
                    "Collegamento o junction non supportati: {}",
                    path.display()
                ));
            }
            if meta.is_dir() {
                check_cancelled(cancelled)?;
                before_io(ScanOperation::ReadDir, &path)?;
                check_cancelled(cancelled)?;
                let mut children = fs::read_dir(&path)
                    .map_err(|e| format!("Preflight {}: {e}", path.display()))?;
                loop {
                    check_cancelled(cancelled)?;
                    before_io(ScanOperation::Next, &path)?;
                    check_cancelled(cancelled)?;
                    let Some(child) = children.next() else {
                        break;
                    };
                    let child = child
                        .map_err(|e| format!("Preflight {}: {e}", path.display()))?
                        .path();
                    let bytes = budget.enqueue(&child)?;
                    stack.push((child, bytes));
                }
            } else if meta.is_file() {
                total = total
                    .checked_add(meta.len())
                    .ok_or("Dimensione complessiva troppo grande")?;
            } else {
                return Err("Tipo di file non supportato".into());
            }
        }
        Ok(total)
    })();
    // On failure queued paths are dropped together with their memory charge.
    for (_, bytes) in stack {
        budget.pending_bytes -= bytes;
    }
    result
}

pub(crate) fn preflight_copy_sizes(
    sources: &[PathBuf],
    cancelled: &AtomicBool,
) -> Result<Vec<u64>, String> {
    let mut budget = ScanBudget::default();
    #[cfg(test)]
    SCAN_TEST_SETTINGS.with(|settings| {
        if let Some(settings) = settings.borrow().as_ref() {
            budget.max_entries = settings.max_entries;
            budget.max_pending_bytes = settings.max_pending_bytes;
        }
    });
    preflight_copy_sizes_with(sources, cancelled, &mut budget, &mut |_operation, _path| {
        #[cfg(test)]
        SCAN_TEST_SETTINGS.with(|settings| match settings.borrow_mut().as_mut() {
            Some(settings) => (settings.before_io)(_operation, _path),
            None => Ok(()),
        })?;
        Ok(())
    })
}
fn preflight_copy_sizes_with(
    sources: &[PathBuf],
    cancelled: &AtomicBool,
    budget: &mut ScanBudget,
    before_io: &mut impl FnMut(ScanOperation, &Path) -> Result<(), String>,
) -> Result<Vec<u64>, String> {
    let mut sizes = Vec::new();
    for source in sources {
        check_cancelled(cancelled)?;
        sizes.push(scan_tree(source, cancelled, budget, before_io)?);
    }
    Ok(sizes)
}

#[cfg(test)]
pub(crate) fn tree_size(source: &Path) -> Result<u64, String> {
    scan_tree(
        source,
        &AtomicBool::new(false),
        &mut ScanBudget::default(),
        &mut |_, _| Ok(()),
    )
}

/// Each write is to a private staging path; a cancellation cleans that staging
/// directory instead of publishing a partial destination.
fn copy_tree_progress(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
) -> Result<(), String> {
    copy_tree_progress_with_sync(source, target, cancelled, on_chunk, &mut |file| {
        file.sync_all()
    })
}

fn copy_tree_progress_with_sync(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
    sync_file: &mut impl FnMut(&fs::File) -> io::Result<()>,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Copia annullata: destinazione incompleta eliminata".into());
    }
    let meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || is_windows_reparse_point(&meta) {
        return Err(format!(
            "Collegamento o junction non copiato: {}",
            source.display()
        ));
    }
    if meta.is_dir() {
        fs::create_dir(target).map_err(|e| e.to_string())?;
        check_cancelled(cancelled)?;
        let mut children = fs::read_dir(source).map_err(|e| e.to_string())?;
        loop {
            check_cancelled(cancelled)?;
            let Some(child) = children.next() else {
                break;
            };
            let child = child.map_err(|e| e.to_string())?;
            copy_tree_progress_with_sync(
                &child.path(),
                &target.join(child.file_name()),
                cancelled,
                on_chunk,
                sync_file,
            )?;
        }
    } else if meta.is_file() {
        let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 256 * 1024];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Copia annullata: destinazione incompleta eliminata".into());
            }
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            on_chunk(n as u64);
        }
        output.flush().map_err(|e| e.to_string())?;
        // Request durable storage before publication. A successful read-back
        // alone can come from the filesystem cache on USB or SMB.
        sync_file(&output).map_err(|e| format!("Sincronizzazione copia non riuscita: {e}"))?;
    } else {
        return Err("Tipo di file non supportato".into());
    }
    Ok(())
}

pub(crate) fn copy_one_progress(
    source: &Path,
    destination_folder: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
) -> Result<(), String> {
    copy_one_progress_with_sync(
        source,
        destination_folder,
        cancelled,
        on_chunk,
        &mut |file| file.sync_all(),
    )
}

fn copy_one_progress_with_sync(
    source: &Path,
    destination_folder: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
    sync_file: &mut impl FnMut(&fs::File) -> io::Result<()>,
) -> Result<(), String> {
    let target = target_path(source, destination_folder)?;
    let name = source.file_name().ok_or("Nome sorgente non valido")?;
    let staging = tempfile::Builder::new()
        .prefix(".veldir-staging-")
        .tempdir_in(destination_folder)
        .map_err(|e| format!("Impossibile creare area temporanea: {e}"))?;
    let staged_target = staging.path().join(name);
    copy_tree_progress_with_sync(source, &staged_target, cancelled, on_chunk, sync_file)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Copia annullata: destinazione incompleta eliminata".into());
    }
    rename_no_replace(&staged_target, &target)
        .map_err(|e| format!("Destinazione già presente o non disponibile: {e}"))?;
    Ok(())
}

/// Atomic rename without replacing a destination: don't use std::fs::rename!
/// On Windows its replacement behavior depends on the filesystem and OS.
#[cfg(windows)]
pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let mut src: Vec<u16> = source.as_os_str().encode_wide().collect();
    let mut dst: Vec<u16> = target.as_os_str().encode_wide().collect();
    if src.contains(&0) || dst.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Percorso non valido",
        ));
    }
    src.push(0);
    dst.push(0);
    // Zero flags: in particular, do NOT pass MOVEFILE_REPLACE_EXISTING.
    let result = unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Linux helper for development/testing; Windows is the distribution target.
#[cfg(target_os = "linux")]
pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let src = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Percorso non valido"))?;
    let dst = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Percorso non valido"))?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            src.as_ptr(),
            libc::AT_FDCWD,
            dst.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(crate) fn rename_no_replace(_source: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Piattaforma non supportata",
    ))
}

/// Preflight every source before a batch starts. This cannot rule out concurrent
/// filesystem changes: a failed batch can therefore be PARTIALLY completed.
fn preflight_batch(
    sources: &[PathBuf],
    destination_folder: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if sources.is_empty() {
        return Err("Nessun elemento selezionato".into());
    }
    check_cancelled(cancelled)?;
    if sources.len() > 100_000 {
        return Err("Preflight interrotto: massimo 100000 elementi per batch".into());
    }
    let mut names = BTreeSet::new();
    for source in sources {
        check_cancelled(cancelled)?;
        // Reject duplicate destination names, even if case differs (Windows semantics).
        let name = source
            .file_name()
            .ok_or("Elemento senza nome")?
            .to_string_lossy()
            .to_lowercase();
        if !names.insert(name) {
            return Err("Due sorgenti hanno lo stesso nome".into());
        }
        fs::symlink_metadata(source).map_err(|e| format!("Sorgente {}: {e}", source.display()))?;
        check_cancelled(cancelled)?;
        target_path(source, destination_folder)?;
    }
    Ok(())
}

/// Preview a numbered bulk-rename plan; extension is preserved for files.
pub fn rename_plan(
    sources: &[PathBuf],
    prefix: &str,
    find: &str,
    replace: &str,
    start: u32,
) -> Result<Vec<(PathBuf, String)>, String> {
    if sources.is_empty() {
        return Err("Seleziona almeno un elemento".into());
    }
    if sources.len() > 10_000 {
        return Err("Massimo 10.000 nomi per operazione".into());
    }
    let mut result = Vec::with_capacity(sources.len());
    for (i, source) in sources.iter().enumerate() {
        let filename = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Nome file non Unicode: operazione non supportata")?;
        let extension = if source.is_file() {
            source.extension().and_then(|s| s.to_str())
        } else {
            None
        };
        let stem = if extension.is_some() {
            source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(filename)
        } else {
            filename
        };
        let replaced = if find.is_empty() {
            stem.to_owned()
        } else {
            stem.replace(find, replace)
        };
        let number = start
            .checked_add(i as u32)
            .ok_or("Contatore troppo grande")?;
        let mut new_name = format!("{prefix}{number:03}_{replaced}");
        if let Some(ext) = extension {
            new_name.push('.');
            new_name.push_str(ext);
        }
        valid_name(&new_name)?;
        result.push((source.clone(), new_name));
    }
    Ok(result)
}

pub(crate) fn preflight_rename_many(changes: &[(PathBuf, String)]) -> Result<(), String> {
    if changes.is_empty() {
        return Err("Nessun elemento da rinominare".into());
    }
    let mut targets = BTreeSet::new();
    for (source, name) in changes {
        valid_name(name)?;
        if source.symlink_metadata().is_err() {
            return Err(format!("Sorgente assente: {}", source.display()));
        }
        let parent = source.parent().ok_or("Percorso senza cartella padre")?;
        let target = parent.join(name);
        if target == *source {
            return Err("Un nome rimane invariato: modifica i parametri".into());
        }
        if !targets.insert(target.to_string_lossy().to_lowercase()) {
            return Err("Due file avrebbero lo stesso nome: operazione interrotta".into());
        }
        if target.symlink_metadata().is_ok() {
            return Err(format!("Destinazione già presente: {}", target.display()));
        }
    }
    Ok(())
}

/// Progress is per item. Cancellation is safe between items; an in-progress file finishes first.
/// Progress for safe file transfers. Byte totals refer to content, not metadata.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_items: usize,
    pub total_items: usize,
}

/// Progress can cancel within an individual file copy (256 KiB chunks).
/// All incomplete destinations remain in staging and are discarded on failure.
pub fn execute_progress<F: Fn(Progress)>(
    job: Job,
    cancelled: &AtomicBool,
    notify: F,
) -> Result<String, String> {
    match job {
        Job::Transfer {
            sources,
            destination,
            cut,
            policy,
        } => crate::transfer::execute(sources, destination, cut, policy, cancelled, notify),
        Job::CopyMany {
            sources,
            destination_folder,
        } => {
            preflight_batch(&sources, &destination_folder, cancelled)?;
            let sizes = preflight_copy_sizes(&sources, cancelled)?;
            let total_bytes = sizes.into_iter().try_fold(0u64, |sum, bytes| {
                sum.checked_add(bytes)
                    .ok_or("Dimensione complessiva troppo grande")
            })?;
            let total_items = sources.len();
            let mut progress = Progress {
                done_bytes: 0,
                total_bytes,
                done_items: 0,
                total_items,
            };
            notify(progress);
            let mut last_report = Instant::now();
            for (index, source) in sources.iter().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(format!(
                        "Annullato dopo {index}/{total_items} elementi; copie già pubblicate conservate"
                    ));
                }
                copy_one_progress(source, &destination_folder, cancelled, &mut |bytes| {
                    progress.done_bytes = progress.done_bytes.saturating_add(bytes);
                    if last_report.elapsed() >= Duration::from_millis(120) {
                        notify(progress);
                        last_report = Instant::now();
                    }
                })
                .map_err(|e| format!("Interrotto dopo {index}/{total_items} elementi: {e}"))?;
                progress.done_items += 1;
                notify(progress);
            }
            Ok(format!("Copiati {total_items} elementi"))
        }
        Job::Copy {
            source,
            destination_folder,
        } => execute_progress(
            Job::CopyMany {
                sources: vec![source],
                destination_folder,
            },
            cancelled,
            notify,
        ),
        Job::MoveMany {
            sources,
            destination_folder,
        } => {
            preflight_batch(&sources, &destination_folder, cancelled)?;
            execute_batch_progress(sources, cancelled, notify, |source| Job::Move {
                source,
                destination_folder: destination_folder.clone(),
            })
        }
        Job::TrashMany { sources } => {
            execute_batch_progress(sources, cancelled, notify, |source| Job::Trash { source })
        }
        Job::RenameMany { changes } => {
            preflight_rename_many(&changes)?;
            let total = changes.len();
            let mut progress = Progress {
                total_items: total,
                ..Progress::default()
            };
            notify(progress);
            for (idx, (source, name)) in changes.into_iter().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(format!(
                        "Annullato dopo {idx}/{total} elementi; modifiche precedenti conservate"
                    ));
                }
                execute(Job::Rename { source, name })?;
                progress.done_items += 1;
                notify(progress);
            }
            Ok(format!("Rinominati {total} elementi"))
        }
        single => {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Operazione annullata".into());
            }
            execute(single)
        }
    }
}

fn execute_batch_progress<F: Fn(Progress)>(
    sources: Vec<PathBuf>,
    cancelled: &AtomicBool,
    notify: F,
    action: impl Fn(PathBuf) -> Job,
) -> Result<String, String> {
    if sources.is_empty() {
        return Err("Nessun elemento selezionato".into());
    }
    let total = sources.len();
    let mut progress = Progress {
        total_items: total,
        ..Progress::default()
    };
    notify(progress);
    for (idx, source) in sources.into_iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(format!(
                "Annullato dopo {idx}/{total} elementi; modifiche precedenti conservate"
            ));
        }
        execute(action(source)).map_err(|e| {
            format!("Interrotto dopo {idx}/{total} elementi, alcuni già modificati: {e}")
        })?;
        progress.done_items += 1;
        notify(progress);
    }
    Ok(format!("Completati {total} elementi"))
}

fn run_batch(sources: Vec<PathBuf>, action: impl Fn(PathBuf) -> Job) -> Result<String, String> {
    if sources.is_empty() {
        return Err("Nessun elemento selezionato".into());
    }
    let total = sources.len();
    for (i, source) in sources.into_iter().enumerate() {
        if let Err(error) = execute(action(source)) {
            return Err(format!(
                "Operazione interrotta dopo {i}/{total} elementi. Alcune modifiche potrebbero essere già state effettuate. Dettaglio: {error}"
            ));
        }
    }
    Ok(format!("Operazione completata su {total} elementi"))
}

pub fn execute(job: Job) -> Result<String, String> {
    match job {
        Job::Transfer {
            sources,
            destination,
            cut,
            policy,
        } => crate::transfer::execute(
            sources,
            destination,
            cut,
            policy,
            &AtomicBool::new(false),
            |_| {},
        ),
        Job::NewFolder { parent, name } => {
            valid_name(&name)?;
            fs::create_dir(parent.join(&name)).map_err(|e| e.to_string())?;
            Ok(format!("Cartella '{name}' creata"))
        }
        Job::Rename { source, name } => {
            valid_name(&name)?;
            let parent = source.parent().ok_or("Elemento senza cartella padre")?;
            let target = parent.join(&name);
            if target == source {
                return Ok("Nome invariato".into());
            }
            rename_no_replace(&source, &target)
                .map_err(|e| format!("Rinomina non riuscita (nessuna sovrascrittura): {e}"))?;
            Ok(format!("Rinominato in '{name}'"))
        }
        Job::Copy {
            source,
            destination_folder,
        } => {
            let target = target_path(&source, &destination_folder)?;
            let name = source.file_name().ok_or("Nome sorgente non valido")?;
            let staging = tempfile::Builder::new()
                .prefix(".veldir-staging-")
                .tempdir_in(&destination_folder)
                .map_err(|e| format!("Impossibile creare una cartella temporanea: {e}"))?;
            let staged_target = staging.path().join(name);
            copy_no_links(&source, &staged_target)?;
            // Commit within the same filesystem. Any conflict leaves the original intact.
            rename_no_replace(&staged_target, &target)
                .map_err(|e| format!("Destinazione non disponibile o già presente: {e}"))?;
            Ok(format!("Copiato in {}", destination_folder.display()))
        }
        Job::Move {
            source,
            destination_folder,
        } => {
            let target = target_path(&source, &destination_folder)?;
            rename_no_replace(&source, &target).map_err(|e| {
                format!(
                    "Spostamento non riuscito: {e}. Tra dischi differenti usa Copia e poi Cestino"
                )
            })?;
            Ok(format!("Spostato in {}", destination_folder.display()))
        }
        Job::Trash { source } => {
            trash::delete(&source).map_err(|e| format!("Impossibile spostare nel Cestino: {e}"))?;
            Ok("Elemento spostato nel Cestino".into())
        }
        Job::CopyMany {
            sources,
            destination_folder,
        } => {
            preflight_batch(&sources, &destination_folder, &AtomicBool::new(false))?;
            run_batch(sources, |source| Job::Copy {
                source,
                destination_folder: destination_folder.clone(),
            })
        }
        Job::MoveMany {
            sources,
            destination_folder,
        } => {
            preflight_batch(&sources, &destination_folder, &AtomicBool::new(false))?;
            run_batch(sources, |source| Job::Move {
                source,
                destination_folder: destination_folder.clone(),
            })
        }
        Job::TrashMany { sources } => run_batch(sources, |source| Job::Trash { source }),
        Job::RenameMany { changes } => {
            preflight_rename_many(&changes)?;
            let total = changes.len();
            for (idx, (source, name)) in changes.into_iter().enumerate() {
                execute(Job::Rename { source, name })
                    .map_err(|e| format!("Rinomina interrotta dopo {idx}/{total}: {e}"))?;
            }
            Ok(format!("Rinominati {total} elementi"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_budget_counts_roots_globally_and_accepts_exact_limit() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.txt");
        let b = root.path().join("b.txt");
        fs::write(&a, b"AAA").unwrap();
        fs::write(&b, b"BBBB").unwrap();
        let sources = vec![a, b];
        let mut budget = ScanBudget {
            max_entries: 1,
            ..ScanBudget::default()
        };
        let error = preflight_copy_sizes_with(
            &sources,
            &AtomicBool::new(false),
            &mut budget,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("massimo 1 elementi"));
        assert_eq!(budget.entries, 1);
        assert_eq!(budget.pending_bytes, 0);
        budget = ScanBudget {
            max_entries: 2,
            ..ScanBudget::default()
        };
        assert_eq!(
            preflight_copy_sizes_with(
                &sources,
                &AtomicBool::new(false),
                &mut budget,
                &mut |_, _| Ok(())
            )
            .unwrap(),
            vec![3, 4]
        );
        assert_eq!(budget.entries, 2);
        assert_eq!(budget.pending_bytes, 0);
    }

    #[test]
    fn scan_pending_memory_and_entry_limits_apply_before_enqueue() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..3 {
            fs::write(root.path().join(format!("child-{n}.txt")), b"X").unwrap();
        }
        let mut budget = ScanBudget {
            max_entries: 2,
            ..ScanBudget::default()
        };
        let error = scan_tree(
            root.path(),
            &AtomicBool::new(false),
            &mut budget,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("massimo 2 elementi"));
        assert_eq!(budget.entries, 2);
        assert_eq!(budget.pending_bytes, 0);
        let max_pending_bytes = ScanBudget::path_bytes(&root.path().join("child-0.txt")).unwrap();
        budget = ScanBudget {
            max_pending_bytes,
            ..ScanBudget::default()
        };
        let error = scan_tree(
            root.path(),
            &AtomicBool::new(false),
            &mut budget,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("limite memoria"));
        assert_eq!(budget.entries, 2);
        assert_eq!(budget.pending_bytes, 0);
    }

    #[test]
    fn scan_cancellation_is_checked_before_each_filesystem_operation() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("child.txt"), b"X").unwrap();
        for operation in [
            ScanOperation::Metadata,
            ScanOperation::ReadDir,
            ScanOperation::Next,
        ] {
            let cancelled = AtomicBool::new(false);
            let mut budget = ScanBudget::default();
            let mut hit = false;
            let error = scan_tree(root.path(), &cancelled, &mut budget, &mut |current, _| {
                if current == operation {
                    hit = true;
                    cancelled.store(true, Ordering::Relaxed);
                }
                Ok(())
            })
            .unwrap_err();
            assert!(hit);
            assert!(error.contains("annullato"), "{error}");
            assert_eq!(budget.pending_bytes, 0);
        }
    }

    #[test]
    fn scan_io_failures_propagate_without_partial_size() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("child.txt"), b"X").unwrap();
        for operation in [
            ScanOperation::Metadata,
            ScanOperation::ReadDir,
            ScanOperation::Next,
        ] {
            let mut budget = ScanBudget::default();
            let error = scan_tree(
                root.path(),
                &AtomicBool::new(false),
                &mut budget,
                &mut |current, _| {
                    if current == operation {
                        Err("simulated access or network error".into())
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
            assert_eq!(error, "simulated access or network error");
            assert_eq!(budget.pending_bytes, 0);
        }
    }

    #[test]
    fn copy_many_global_budget_failure_does_not_publish_first_source() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, b"FIRST").unwrap();
        fs::write(&second, b"SECOND").unwrap();
        let result = with_scan_test_settings(
            1,
            1024 * 1024,
            |_, _| Ok(()),
            || {
                execute_progress(
                    Job::CopyMany {
                        sources: vec![first.clone(), second.clone()],
                        destination_folder: destination.clone(),
                    },
                    &AtomicBool::new(false),
                    |_| {},
                )
            },
        )
        .unwrap_err();
        assert!(result.contains("massimo 1 elementi"));
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        assert_eq!(fs::read(first).unwrap(), b"FIRST");
        assert_eq!(fs::read(second).unwrap(), b"SECOND");
    }

    #[test]
    fn sync_failure_prevents_publication_and_preserves_source() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        let folder = root.path().join("source-folder");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("child.txt"), b"KEEP CHILD").unwrap();
        let file = root.path().join("source.txt");
        fs::write(&file, b"KEEP SOURCE").unwrap();
        fs::write(destination.join("existing.txt"), b"KEEP DESTINATION").unwrap();
        for source in [&file, &folder] {
            let mut sync_calls = 0;
            let error = copy_one_progress_with_sync(
                source,
                &destination,
                &AtomicBool::new(false),
                &mut |_| {},
                &mut |staged| {
                    sync_calls += 1;
                    assert!(staged.metadata().unwrap().len() > 0);
                    Err(io::Error::from(io::ErrorKind::StorageFull))
                },
            )
            .unwrap_err();
            assert_eq!(sync_calls, 1);
            assert!(error.contains("Sincronizzazione copia non riuscita"));
            assert!(!destination.join(source.file_name().unwrap()).exists());
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        }
        assert_eq!(fs::read(file).unwrap(), b"KEEP SOURCE");
        assert_eq!(fs::read(folder.join("child.txt")).unwrap(), b"KEEP CHILD");
        assert_eq!(
            fs::read(destination.join("existing.txt")).unwrap(),
            b"KEEP DESTINATION"
        );
    }

    #[test]
    #[ignore = "Misurazione esplicita, crea 1000 e 10000 file temporanei"]
    fn benchmark_large_listing() {
        for count in [1000, 10000] {
            let dir = tempfile::tempdir().unwrap();
            for n in 0..count {
                fs::write(dir.path().join(format!("file-{n:05}.txt")), b"benchmark").unwrap();
            }
            let mut samples = Vec::new();
            for _ in 0..3 {
                let now = Instant::now();
                assert_eq!(list(dir.path()).unwrap().len(), count);
                samples.push(now.elapsed().as_millis());
            }
            println!("Veldir listing {count} files: milliseconds {samples:?}");
        }
    }

    #[test]
    fn bulk_rename_preflight_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "AAA").unwrap();
        fs::write(&b, "BBB").unwrap();
        let plan = rename_plan(&[a.clone(), b.clone()], "pic_", "", "", 1).unwrap();
        assert_eq!(plan[0].1, "pic_001_a.txt");
        assert_eq!(plan[1].1, "pic_002_b.txt");
        fs::write(dir.path().join("pic_002_b.txt"), "ORIGINAL").unwrap();
        assert!(execute(Job::RenameMany { changes: plan }).is_err());
        assert_eq!(fs::read_to_string(a).unwrap(), "AAA");
        assert_eq!(fs::read_to_string(b).unwrap(), "BBB");
    }
    #[test]
    fn cancellation_keeps_prior_items_and_leaves_next_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        let a = from.join("a.txt");
        let b = from.join("b.txt");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let cancelled = AtomicBool::new(false);
        let result = execute_progress(
            Job::CopyMany {
                sources: vec![a, b],
                destination_folder: to.clone(),
            },
            &cancelled,
            |progress| {
                if progress.done_items == 1 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(result.is_err());
        assert!(to.join("a.txt").exists());
        assert!(!to.join("b.txt").exists());
    }
    #[test]
    fn large_file_cancellation_keeps_source_and_removes_staging() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        let source = from.join("large.bin");
        fs::write(&source, vec![7u8; 3 * 1024 * 1024]).unwrap();
        let cancelled = AtomicBool::new(false);
        // The engine checks cancellation every 256 KiB.
        let mut seen = 0u64;
        let result = copy_one_progress(&source, &to, &cancelled, &mut |bytes| {
            seen += bytes;
            if seen >= 256 * 1024 {
                cancelled.store(true, Ordering::Relaxed);
            }
        });
        assert!(result.is_err());
        assert_eq!(fs::metadata(&source).unwrap().len(), 3 * 1024 * 1024);
        assert_eq!(fs::read_dir(&to).unwrap().count(), 0);
    }
    #[test]
    fn byte_progress_reaches_total_on_copy() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        let source = from.join("data.bin");
        fs::write(&source, vec![11u8; 550_000]).unwrap();
        let cancelled = AtomicBool::new(false);
        let progress = std::cell::Cell::new(Progress::default());
        execute_progress(
            Job::CopyMany {
                sources: vec![source.clone()],
                destination_folder: to.clone(),
            },
            &cancelled,
            |value| progress.set(value),
        )
        .unwrap();
        let p = progress.get();
        assert_eq!(p.total_bytes, 550_000);
        assert_eq!(p.done_bytes, 550_000);
        assert_eq!(p.done_items, 1);
        assert_eq!(p.total_items, 1);
        assert_eq!(
            fs::read(&source).unwrap(),
            fs::read(to.join("data.bin")).unwrap()
        );
    }
    #[test]
    fn windows_bad_names_rejected() {
        for name in [
            "", "..", "foo/bar", "CON.txt", "LPT1", "test.", "a\\b", "hi ",
        ] {
            assert!(valid_name(name).is_err(), "{name:?}");
        }
        for name in ["Foto vacanza", "Report 2026.pdf", "progetto_v1"] {
            assert!(valid_name(name).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn no_overwrite_when_target_exists() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("a.txt");
        fs::write(&source, "originale").unwrap();
        let err = execute(Job::Copy {
            source: source.clone(),
            destination_folder: temp.path().to_path_buf(),
        });
        assert!(err.is_err());
        assert_eq!(fs::read_to_string(&source).unwrap(), "originale");
    }

    #[test]
    fn create_rename_copy_move_without_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        execute(Job::NewFolder {
            parent: temp.path().to_path_buf(),
            name: "Nuova".into(),
        })
        .unwrap();
        execute(Job::Rename {
            source: temp.path().join("Nuova"),
            name: "Rinominata".into(),
        })
        .unwrap();
        let dest = temp.path().join("Dest");
        fs::create_dir(&dest).unwrap();
        execute(Job::Copy {
            source: temp.path().join("Rinominata"),
            destination_folder: dest.clone(),
        })
        .unwrap();
        assert!(dest.join("Rinominata").is_dir());
        fs::write(temp.path().join("report.txt"), "abc").unwrap();
        execute(Job::Move {
            source: temp.path().join("report.txt"),
            destination_folder: dest.clone(),
        })
        .unwrap();
        assert_eq!(fs::read_to_string(dest.join("report.txt")).unwrap(), "abc");
        assert!(!temp.path().join("report.txt").exists());
    }

    #[test]
    fn copy_files_preserve_contents_and_leave_no_staging() {
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("from");
        let to = temp.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        fs::write(from.join("text.txt"), "contenuto 🌟").unwrap();
        execute(Job::Copy {
            source: from.join("text.txt"),
            destination_folder: to.clone(),
        })
        .unwrap();
        assert_eq!(
            fs::read_to_string(to.join("text.txt")).unwrap(),
            "contenuto 🌟"
        );
        let elements: Vec<_> = fs::read_dir(&to)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(elements.len(), 1, "Nessun residuo temporaneo previsto");
    }

    #[test]
    fn reject_copying_folder_into_descendant() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();
        let result = execute(Job::Copy {
            source: parent,
            destination_folder: child.clone(),
        });
        assert!(result.is_err());
        assert!(!child.join("parent").exists());
    }

    #[cfg(unix)]
    #[test]
    fn reject_symlinks_inside_copied_tree() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("source");
        let to = temp.path().join("dest");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        fs::write(from.join("ok.txt"), "ok").unwrap();
        symlink(from.join("ok.txt"), from.join("alias.txt")).unwrap();
        let result = execute(Job::Copy {
            source: from,
            destination_folder: to.clone(),
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_dir(to).unwrap().count(),
            0,
            "Un errore non deve pubblicare copie parziali"
        );
    }

    #[test]
    fn rename_no_replace_prevents_existing_file_loss() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        assert!(
            execute(Job::Rename {
                source: a.clone(),
                name: "b.txt".into()
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(a).unwrap(), "A");
        assert_eq!(fs::read_to_string(b).unwrap(), "B");
    }
    #[test]
    fn batch_copy_refuses_conflicts_without_partial_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        let dest = tmp.path().join("dest");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(a.join("first.txt"), "one").unwrap();
        fs::write(b.join("second.txt"), "two").unwrap();
        fs::write(dest.join("second.txt"), "original").unwrap();
        let result = execute(Job::CopyMany {
            sources: vec![a.join("first.txt"), b.join("second.txt")],
            destination_folder: dest.clone(),
        });
        assert!(result.is_err());
        assert!(!dest.join("first.txt").exists());
        assert_eq!(
            fs::read_to_string(dest.join("second.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn batch_copy_moves_multiple_files_without_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        let to = tmp.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        for name in ["a.txt", "b.txt"] {
            fs::write(from.join(name), name).unwrap();
        }
        execute(Job::CopyMany {
            sources: vec![from.join("a.txt"), from.join("b.txt")],
            destination_folder: to.clone(),
        })
        .unwrap();
        assert_eq!(fs::read_to_string(to.join("a.txt")).unwrap(), "a.txt");
        assert_eq!(fs::read_to_string(to.join("b.txt")).unwrap(), "b.txt");
    }
}
