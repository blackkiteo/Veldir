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
            .and_then(|n| n.checked_add(std::mem::size_of::<(PathBuf, usize, usize)>()))
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
    let mut stack = vec![(source.to_path_buf(), bytes, 0usize)];
    let mut total = 0u64;
    let result = (|| {
        while let Some((path, bytes, parent_directories)) = stack.pop() {
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
                let directory_depth = next_directory_depth(parent_directories)?;
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
                    stack.push((child, bytes, directory_depth));
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
    for (_, bytes, _) in stack {
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

/// Counts simultaneously held source-directory guards. Files add no guard.
const MAX_DIRECTORY_GUARDS: usize = 256;

fn directory_depth_limit() -> usize {
    #[cfg(test)]
    if let Some(limit) = DIRECTORY_COPY_TEST_SETTINGS.with(|settings| {
        settings
            .borrow()
            .as_ref()
            .map(|settings| settings.max_depth)
    }) {
        return limit;
    }
    MAX_DIRECTORY_GUARDS
}

fn next_directory_depth(parent_directories: usize) -> Result<usize, String> {
    let depth = parent_directories
        .checked_add(1)
        .ok_or("Profondità directory troppo grande")?;
    let limit = directory_depth_limit();
    if depth > limit {
        return Err(format!(
            "Profondità directory non supportata: massimo {limit} directory contemporanee"
        ));
    }
    Ok(depth)
}

/// Pin a directory while its descendants are copied. This protects the visited
/// directory on Windows, not unvisited ancestors or a universal handle-relative
/// traversal: read_dir and child opens still resolve paths.
fn open_copy_directory(source: &Path) -> Result<(fs::File, SystemTime), String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(0);
    }
    let guard = options
        .open(source)
        .map_err(|e| format!("Apertura directory protetta {}: {e}", source.display()))?;
    let snapshot = guard
        .metadata()
        .map_err(|e| format!("Metadata handle directory: {e}"))?;
    if !snapshot.is_dir()
        || snapshot.file_type().is_symlink()
        || is_windows_reparse_point(&snapshot)
    {
        return Err(format!(
            "Handle sorgente non è una directory senza reparse point: {}",
            source.display()
        ));
    }
    let modified = snapshot
        .modified()
        .map_err(|e| format!("Data modifica handle directory non disponibile: {e}"))?;
    Ok((guard, modified))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyDirectoryStage {
    BeforeOpen,
    Validated,
    BeforeReadDir,
    BeforeNext,
    BeforeFinal,
}
#[cfg(test)]
struct DirectoryCopyTestSettings {
    max_depth: usize,
    hook: Box<dyn FnMut(CopyDirectoryStage, &Path) -> Result<(), String>>,
}
#[cfg(test)]
thread_local! {
    static DIRECTORY_COPY_TEST_SETTINGS: std::cell::RefCell<Option<DirectoryCopyTestSettings>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn with_directory_copy_test_settings<T>(
    max_depth: usize,
    hook: impl FnMut(CopyDirectoryStage, &Path) -> Result<(), String> + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<DirectoryCopyTestSettings>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DIRECTORY_COPY_TEST_SETTINGS.with(|settings| {
                settings.replace(self.0.take());
            });
        }
    }
    let previous = DIRECTORY_COPY_TEST_SETTINGS.with(|settings| {
        settings.replace(Some(DirectoryCopyTestSettings {
            max_depth,
            hook: Box::new(hook),
        }))
    });
    let _restore = Restore(previous);
    action()
}
#[cfg(test)]
fn copy_directory_checkpoint(stage: CopyDirectoryStage, source: &Path) -> Result<(), String> {
    DIRECTORY_COPY_TEST_SETTINGS.with(|settings| match settings.borrow_mut().as_mut() {
        Some(settings) => (settings.hook)(stage, source),
        None => Ok(()),
    })
}

/// The capture point is this open, not the earlier path-based preflight.
/// Windows refuses data writers and delete/rename sharing while this handle
/// lives and opens the leaf reparse point itself rather than following it.
/// Unvisited ancestors and pre-open replacement by another regular file
/// are outside this guarantee. Other platforms do not provide this sharing lock.
fn open_copy_source(source: &Path) -> Result<(fs::File, fs::Metadata, SystemTime), String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(0); // SECURITY_ANONYMOUS; std adds SECURITY_SQOS_PRESENT.
    }
    let input = options
        .open(source)
        .map_err(|e| format!("Apertura sorgente protetta {}: {e}", source.display()))?;
    let snapshot = input
        .metadata()
        .map_err(|e| format!("Metadata handle sorgente: {e}"))?;
    if !snapshot.is_file()
        || snapshot.file_type().is_symlink()
        || is_windows_reparse_point(&snapshot)
    {
        return Err(format!(
            "Handle sorgente non è un file regolare senza reparse point: {}",
            source.display()
        ));
    }
    let modified = snapshot
        .modified()
        .map_err(|e| format!("Data modifica handle sorgente non disponibile: {e}"))?;
    Ok((input, snapshot, modified))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopySourceStage {
    BeforeOpen,
    Validated,
}
#[cfg(test)]
thread_local! {
    static COPY_SOURCE_HOOK: std::cell::RefCell<Option<Box<dyn FnMut(CopySourceStage, &Path) -> Result<(), String>>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn with_copy_source_hook<T>(
    hook: impl FnMut(CopySourceStage, &Path) -> Result<(), String> + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Box<dyn FnMut(CopySourceStage, &Path) -> Result<(), String>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            COPY_SOURCE_HOOK.with(|hook| {
                hook.replace(self.0.take());
            });
        }
    }
    let previous = COPY_SOURCE_HOOK.with(|current| current.replace(Some(Box::new(hook))));
    let _restore = Restore(previous);
    action()
}
#[cfg(test)]
fn copy_source_checkpoint(stage: CopySourceStage, source: &Path) -> Result<(), String> {
    COPY_SOURCE_HOOK.with(|hook| match hook.borrow_mut().as_mut() {
        Some(hook) => hook(stage, source),
        None => Ok(()),
    })
}

/// Independently owned frames keep visited ancestor guards alive on the heap.
/// Dropping the Vec on failure closes every iterator and guard without recursion.
struct DirectoryCopyFrame {
    source: PathBuf,
    target: PathBuf,
    depth: usize,
    children: Option<fs::ReadDir>,
    guard: fs::File,
    modified: SystemTime,
}

fn open_directory_copy_frame(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    parent_directories: usize,
) -> Result<DirectoryCopyFrame, String> {
    // Enforce before opening the next guard, including source trees that grew
    // after preflight. Files never call this function and add no guard.
    let depth = next_directory_depth(parent_directories)?;
    check_cancelled(cancelled)?;
    #[cfg(test)]
    copy_directory_checkpoint(CopyDirectoryStage::BeforeOpen, source)?;
    check_cancelled(cancelled)?;
    let (guard, modified) = open_copy_directory(source)?;
    #[cfg(test)]
    copy_directory_checkpoint(CopyDirectoryStage::Validated, source)?;
    check_cancelled(cancelled)?;
    fs::create_dir(target).map_err(|e| e.to_string())?;
    #[cfg(test)]
    copy_directory_checkpoint(CopyDirectoryStage::BeforeReadDir, source)?;
    check_cancelled(cancelled)?;
    let children = fs::read_dir(source).map_err(|e| e.to_string())?;
    Ok(DirectoryCopyFrame {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        depth,
        children: Some(children),
        guard,
        modified,
    })
}

fn copy_source_metadata(source: &Path, cancelled: &AtomicBool) -> Result<fs::Metadata, String> {
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
    if !meta.is_file() && !meta.is_dir() {
        return Err("Tipo di file non supportato".into());
    }
    Ok(meta)
}

/// Each write is to a private staging path; cancellation or failure removes
/// that staging. DFS frames use heap storage, so directory depth does not grow
/// the Rust call stack. The root counts as one guard and files count as zero.
fn copy_tree_progress_with_sync(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
    sync_file: &mut impl FnMut(&fs::File) -> io::Result<()>,
) -> Result<(), String> {
    let meta = copy_source_metadata(source, cancelled)?;
    if meta.is_file() {
        return copy_regular_file_progress_with_sync(
            source, target, cancelled, on_chunk, sync_file,
        );
    }
    let mut frames = vec![open_directory_copy_frame(source, target, cancelled, 0)?];
    while let Some(frame) = frames.last_mut() {
        check_cancelled(cancelled)?;
        #[cfg(test)]
        copy_directory_checkpoint(CopyDirectoryStage::BeforeNext, &frame.source)?;
        check_cancelled(cancelled)?;
        let child = frame
            .children
            .as_mut()
            .ok_or("Enumeratore directory non disponibile")?
            .next();
        if let Some(child) = child {
            let child = child.map_err(|e| e.to_string())?;
            let source = child.path();
            let target = frame.target.join(child.file_name());
            let parent_depth = frame.depth;
            let meta = copy_source_metadata(&source, cancelled)?;
            if meta.is_file() {
                copy_regular_file_progress_with_sync(
                    &source, &target, cancelled, on_chunk, sync_file,
                )?;
            } else {
                let child_frame =
                    open_directory_copy_frame(&source, &target, cancelled, parent_depth)?;
                frames.push(child_frame);
            }
        } else {
            // Close enumeration before final checks while retaining this guard
            // and every ancestor guard. Pop only after validation succeeds.
            drop(frame.children.take());
            check_cancelled(cancelled)?;
            #[cfg(test)]
            copy_directory_checkpoint(CopyDirectoryStage::BeforeFinal, &frame.source)?;
            check_cancelled(cancelled)?;
            let current = frame
                .guard
                .metadata()
                .map_err(|e| format!("Controllo finale handle directory: {e}"))?;
            let current_modified = current
                .modified()
                .map_err(|e| format!("Data finale handle directory non disponibile: {e}"))?;
            if !current.is_dir()
                || current.file_type().is_symlink()
                || is_windows_reparse_point(&current)
                || current_modified != frame.modified
            {
                return Err(
                    "Directory sorgente cambiata dopo l'apertura protetta; copia non pubblicata"
                        .into(),
                );
            }
            frames.pop();
        }
    }
    Ok(())
}

/// File buffering stays on the heap; directory traversal is iterative even
/// in debug builds on a 2 MiB Windows thread stack.
fn copy_regular_file_progress_with_sync(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    on_chunk: &mut impl FnMut(u64),
    sync_file: &mut impl FnMut(&fs::File) -> io::Result<()>,
) -> Result<(), String> {
    check_cancelled(cancelled)?;
    #[cfg(test)]
    copy_source_checkpoint(CopySourceStage::BeforeOpen, source)?;
    check_cancelled(cancelled)?;
    let (mut input, snapshot, modified) = open_copy_source(source)?;
    #[cfg(test)]
    copy_source_checkpoint(CopySourceStage::Validated, source)?;
    check_cancelled(cancelled)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut copied_bytes = 0u64;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Copia annullata: destinazione incompleta eliminata".into());
        }
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        copied_bytes = copied_bytes
            .checked_add(n as u64)
            .ok_or("Conteggio byte copiati troppo grande")?;
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        on_chunk(n as u64);
    }
    output.flush().map_err(|e| e.to_string())?;
    // Request durable storage before publication. A successful read-back
    // alone can come from the filesystem cache on USB or SMB.
    sync_file(&output).map_err(|e| format!("Sincronizzazione copia non riuscita: {e}"))?;
    check_cancelled(cancelled)?;
    let current = input
        .metadata()
        .map_err(|e| format!("Controllo finale handle sorgente: {e}"))?;
    let current_modified = current
        .modified()
        .map_err(|e| format!("Data finale handle sorgente non disponibile: {e}"))?;
    if !current.is_file()
        || current.file_type().is_symlink()
        || is_windows_reparse_point(&current)
        || copied_bytes != snapshot.len()
        || current.len() != snapshot.len()
        || current_modified != modified
    {
        return Err("Sorgente cambiata dopo l'apertura protetta; copia non pubblicata".into());
    }
    // Explicitly retain the protected handle through output sync and final
    // checks; publication happens later and does not delete the source.
    drop(input);
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
            let success = format!("Copiato in {}", destination_folder.display());
            // The synchronous API shares the full preflight but does not expose
            // an external cancellation token or progress callback.
            execute_progress(
                Job::Copy {
                    source,
                    destination_folder,
                },
                &AtomicBool::new(false),
                |_| {},
            )
            .map(|_| success)
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
            let total = sources.len();
            // Delegate the entire batch so roots share one scan budget and no
            // source is published before every source has passed preflight.
            execute_progress(
                Job::CopyMany {
                    sources,
                    destination_folder,
                },
                &AtomicBool::new(false),
                |_| {},
            )
            .map(|_| format!("Operazione completata su {total} elementi"))
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

    #[cfg(windows)]
    fn assert_directory_guard_allows_reader_and_denies_rename_delete(source: &Path) {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        let reader = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(source)
            .expect("A compatible directory reader must be allowed");
        assert!(reader.metadata().unwrap().is_dir());
        drop(reader);
        // A nonempty remove_dir failure would not demonstrate protection.
        // Request DELETE access without setting a deletion disposition instead.
        let denied = fs::OpenOptions::new()
            .access_mode(DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(source)
            .unwrap_err();
        assert_eq!(
            denied.raw_os_error(),
            Some(32),
            "Expected directory DELETE sharing violation: {denied}"
        );
        let renamed = source.with_extension("denied-directory-rename");
        assert!(!renamed.exists());
        assert!(
            fs::rename(source, &renamed).is_err(),
            "Directory rename must be denied while guarded"
        );
        assert!(source.is_dir());
        assert!(!renamed.exists());
    }

    #[cfg(windows)]
    fn assert_directory_guard_released(source: &Path) {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        let deletion_access = fs::OpenOptions::new()
            .access_mode(DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(source)
            .expect("Directory DELETE access must be released after copying ends");
        drop(deletion_access);
        let renamed = source.with_extension("released-directory-rename");
        assert!(!renamed.exists());
        fs::rename(source, &renamed).expect("Directory rename must succeed after guard release");
        fs::rename(&renamed, source).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_file_replacement_is_rejected_before_output_creation() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let saved = root.path().join("saved-original");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("child.txt"), b"KEEP CHILD").unwrap();
        let archived = saved.clone();
        let validated = std::rc::Rc::new(std::cell::Cell::new(false));
        let hit = validated.clone();
        let result = with_directory_copy_test_settings(
            MAX_DIRECTORY_GUARDS,
            move |stage, path| {
                if stage == CopyDirectoryStage::BeforeOpen {
                    fs::rename(path, &archived).unwrap();
                    fs::write(path, b"KEEP REPLACEMENT").unwrap();
                } else if stage == CopyDirectoryStage::Validated {
                    hit.set(true);
                }
                Ok(())
            },
            || copy_one_progress(&source, &destination, &AtomicBool::new(false), &mut |_| {}),
        );
        let error = result.unwrap_err();
        assert!(error.contains("non è una directory"), "{error}");
        assert!(!validated.get());
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        assert_eq!(fs::read(&source).unwrap(), b"KEEP REPLACEMENT");
        assert_eq!(fs::read(saved.join("child.txt")).unwrap(), b"KEEP CHILD");
        assert_source_handle_released(&source);
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_symlink_replacement_is_rejected_without_following_target() {
        use std::os::windows::fs::symlink_dir;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let saved = root.path().join("saved-original");
        let linked = root.path().join("link-target");
        let destination = root.path().join("destination");
        for directory in [&source, &linked, &destination] {
            fs::create_dir(directory).unwrap();
        }
        fs::write(source.join("original.txt"), b"KEEP ORIGINAL").unwrap();
        fs::write(linked.join("target.txt"), b"KEEP LINK TARGET").unwrap();
        let archived = saved.clone();
        let reference = linked.clone();
        let error = with_directory_copy_test_settings(MAX_DIRECTORY_GUARDS, move |stage, path| {
            if stage == CopyDirectoryStage::BeforeOpen {
                fs::rename(path, &archived).unwrap();
                symlink_dir(&reference, path).expect("Directory symlink creation requires Windows CreateSymbolicLink privilege or Developer Mode; no automatic skip");
            } else if stage == CopyDirectoryStage::Validated { panic!("A directory reparse point must not become a validated guard"); }
            Ok(())
        }, || copy_one_progress(&source, &destination, &AtomicBool::new(false), &mut |_| {})).unwrap_err();
        assert!(
            error.contains("non è una directory senza reparse point"),
            "{error}"
        );
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        assert_eq!(
            fs::read(saved.join("original.txt")).unwrap(),
            b"KEEP ORIGINAL"
        );
        assert_eq!(
            fs::read(linked.join("target.txt")).unwrap(),
            b"KEEP LINK TARGET"
        );
        assert!(
            fs::symlink_metadata(source)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_ancestor_directory_guards_live_through_child_chunks_sync_and_final_checks() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let nested = source.join("nested");
        let destination = root.path().join("destination");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(&destination).unwrap();
        let bytes = vec![0x5a; 800_000];
        fs::write(nested.join("child.bin"), &bytes).unwrap();
        let guard_paths = vec![source.clone(), nested.clone()];
        let finals = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = finals.clone();
        let mut chunks = 0;
        let mut syncs = 0;
        with_directory_copy_test_settings(
            2,
            move |stage, path| {
                if stage == CopyDirectoryStage::Validated
                    || stage == CopyDirectoryStage::BeforeFinal
                {
                    assert_directory_guard_allows_reader_and_denies_rename_delete(path);
                }
                if stage == CopyDirectoryStage::BeforeFinal {
                    recorded.borrow_mut().push(path.to_path_buf());
                }
                Ok(())
            },
            || {
                copy_one_progress_with_sync(
                    &source,
                    &destination,
                    &AtomicBool::new(false),
                    &mut |_| {
                        chunks += 1;
                        for directory in &guard_paths {
                            assert_directory_guard_allows_reader_and_denies_rename_delete(
                                directory,
                            );
                        }
                    },
                    &mut |staged| {
                        syncs += 1;
                        for directory in &guard_paths {
                            assert_directory_guard_allows_reader_and_denies_rename_delete(
                                directory,
                            );
                        }
                        staged.sync_all()
                    },
                )
            },
        )
        .unwrap();
        assert!(chunks >= 2);
        assert_eq!(syncs, 1);
        assert_eq!(*finals.borrow(), vec![nested.clone(), source.clone()]);
        assert_eq!(fs::read(nested.join("child.bin")).unwrap(), bytes);
        assert_eq!(
            fs::read(destination.join("source").join("nested").join("child.bin")).unwrap(),
            bytes
        );
        assert_directory_guard_released(&source);
        assert_directory_guard_released(&nested);
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_guards_release_and_staging_cleans_on_error_cancel_enum_and_sync_failure() {
        for scenario in 0..6 {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source");
            let nested = source.join("nested");
            let destination = root.path().join("destination");
            fs::create_dir_all(&nested).unwrap();
            fs::create_dir(&destination).unwrap();
            let bytes = vec![0x33; 600_000];
            fs::write(nested.join("child.bin"), &bytes).unwrap();
            let cancelled = std::sync::Arc::new(AtomicBool::new(false));
            let injected_cancel = cancelled.clone();
            let nested_path = nested.clone();
            let injected = std::rc::Rc::new(std::cell::Cell::new(false));
            let hit = injected.clone();
            let result = with_directory_copy_test_settings(
                2,
                move |stage, path| {
                    if path == nested_path {
                        if stage != CopyDirectoryStage::BeforeOpen {
                            assert_directory_guard_allows_reader_and_denies_rename_delete(path);
                        }
                        if scenario == 0 && stage == CopyDirectoryStage::Validated {
                            hit.set(true);
                            return Err("injected guarded-directory error".into());
                        }
                        if scenario == 2 && stage == CopyDirectoryStage::BeforeNext {
                            hit.set(true);
                            return Err("injected directory enumeration error".into());
                        }
                        if scenario == 4 && stage == CopyDirectoryStage::Validated {
                            hit.set(true);
                            injected_cancel.store(true, Ordering::Relaxed);
                        }
                        if scenario == 5 && stage == CopyDirectoryStage::BeforeFinal {
                            hit.set(true);
                            return Err("injected directory final-check error".into());
                        }
                    }
                    Ok(())
                },
                || {
                    copy_one_progress_with_sync(
                        &source,
                        &destination,
                        &cancelled,
                        &mut |_| {
                            assert_directory_guard_allows_reader_and_denies_rename_delete(&source);
                            assert_directory_guard_allows_reader_and_denies_rename_delete(&nested);
                            if scenario == 1 {
                                cancelled.store(true, Ordering::Relaxed);
                            }
                        },
                        &mut |staged| {
                            assert_directory_guard_allows_reader_and_denies_rename_delete(&source);
                            assert_directory_guard_allows_reader_and_denies_rename_delete(&nested);
                            if scenario == 3 {
                                Err(io::Error::from(io::ErrorKind::StorageFull))
                            } else {
                                staged.sync_all()
                            }
                        },
                    )
                },
            );
            assert!(result.is_err(), "scenario {scenario}");
            if [0, 2, 4, 5].contains(&scenario) {
                assert!(injected.get());
            }
            assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
            assert_eq!(fs::read(nested.join("child.bin")).unwrap(), bytes);
            assert_directory_guard_released(&source);
            assert_directory_guard_released(&nested);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_final_handle_timestamp_change_prevents_publication() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_WRITE_ATTRIBUTES,
        };
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let changed = fs::metadata(&source).unwrap().modified().unwrap() + Duration::from_secs(120);
        let error = with_directory_copy_test_settings(
            1,
            move |stage, path| {
                if stage == CopyDirectoryStage::BeforeFinal {
                    assert_directory_guard_allows_reader_and_denies_rename_delete(path);
                    let attributes = fs::OpenOptions::new()
                        .access_mode(FILE_WRITE_ATTRIBUTES)
                        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                        .open(path)
                        .unwrap();
                    attributes
                        .set_times(fs::FileTimes::new().set_modified(changed))
                        .unwrap();
                    drop(attributes);
                }
                Ok(())
            },
            || copy_one_progress(&source, &destination, &AtomicBool::new(false), &mut |_| {}),
        )
        .unwrap_err();
        assert!(error.contains("Directory sorgente cambiata"), "{error}");
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), changed);
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        assert_directory_guard_released(&source);
    }

    #[test]
    fn directory_depth_limit_is_shared_by_preflight_and_copier_and_files_add_no_guard() {
        for limit in [1, 2] {
            let root = tempfile::tempdir().unwrap();
            let first = root.path().join("first.txt");
            let source = root.path().join("source");
            let nested = source.join("nested");
            let destination = root.path().join("destination");
            fs::create_dir_all(&nested).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(&first, b"FIRST").unwrap();
            fs::write(nested.join("child.txt"), b"CHILD").unwrap();
            let opens = std::rc::Rc::new(std::cell::Cell::new(0));
            let recorded = opens.clone();
            let result = with_directory_copy_test_settings(
                limit,
                move |stage, _| {
                    if stage == CopyDirectoryStage::BeforeOpen {
                        recorded.set(recorded.get() + 1);
                    }
                    Ok(())
                },
                || {
                    execute(Job::CopyMany {
                        sources: vec![first.clone(), source.clone()],
                        destination_folder: destination.clone(),
                    })
                },
            );
            if limit == 1 {
                assert!(result.unwrap_err().contains("massimo 1 directory"));
                assert_eq!(
                    opens.get(),
                    0,
                    "Entire batch must fail preflight before opening copy guards"
                );
                assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
                let error = with_directory_copy_test_settings(
                    1,
                    |_, _| Ok(()),
                    || {
                        copy_one_progress(
                            &source,
                            &destination,
                            &AtomicBool::new(false),
                            &mut |_| {},
                        )
                    },
                )
                .unwrap_err();
                assert!(error.contains("massimo 1 directory"));
                assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
            } else {
                result.unwrap();
                assert_eq!(opens.get(), 2);
                assert_eq!(fs::read(destination.join("first.txt")).unwrap(), b"FIRST");
                assert_eq!(
                    fs::read(destination.join("source").join("nested").join("child.txt")).unwrap(),
                    b"CHILD"
                );
            }
            assert_eq!(fs::read(first).unwrap(), b"FIRST");
            assert_eq!(fs::read(nested.join("child.txt")).unwrap(), b"CHILD");
        }
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("file.txt");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"FILE ADDS NO DIRECTORY GUARD").unwrap();
        with_directory_copy_test_settings(
            0,
            |_, _| panic!("A file must not open a directory guard"),
            || {
                execute(Job::Copy {
                    source: source.clone(),
                    destination_folder: destination.clone(),
                })
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(destination.join("file.txt")).unwrap(),
            b"FILE ADDS NO DIRECTORY GUARD"
        );
    }

    #[test]
    fn copier_rechecks_depth_for_directories_added_after_preflight() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let nested = source.join("nested");
        let leaf = nested.join("child.txt");
        let later = nested.join("later");
        let destination = root.path().join("destination");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(&leaf, b"KEEP").unwrap();
        let trigger = leaf.clone();
        let new_directory = later.clone();
        let changed = std::rc::Rc::new(std::cell::Cell::new(false));
        let injected = changed.clone();
        let guarded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = guarded.clone();
        let result = with_directory_copy_test_settings(
            2,
            move |stage, path| {
                if stage == CopyDirectoryStage::BeforeOpen {
                    recorded.borrow_mut().push(path.to_path_buf());
                }
                Ok(())
            },
            || {
                with_scan_test_settings(
                    100,
                    1024 * 1024,
                    move |operation, path| {
                        if operation == ScanOperation::Metadata
                            && path == trigger
                            && !injected.replace(true)
                        {
                            fs::create_dir(&new_directory).unwrap();
                        }
                        Ok(())
                    },
                    || {
                        execute(Job::Copy {
                            source: source.clone(),
                            destination_folder: destination.clone(),
                        })
                    },
                )
            },
        );
        assert!(changed.get());
        assert!(result.unwrap_err().contains("massimo 2 directory"));
        assert_eq!(*guarded.borrow(), vec![source.clone(), nested.clone()]);
        assert!(later.is_dir());
        assert_eq!(fs::read(leaf).unwrap(), b"KEEP");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn directory_copy_preserves_unicode_empty_directories_and_empty_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("sorgente résumé 漢字");
        let nested = source.join("vuota résumé 漢字");
        let destination = root.path().join("destination");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("vuoto.txt"), b"").unwrap();
        fs::write(source.join("contenuto.txt"), b"CONTENTS").unwrap();
        with_directory_copy_test_settings(
            2,
            |_, _| Ok(()),
            || {
                execute(Job::Copy {
                    source: source.clone(),
                    destination_folder: destination.clone(),
                })
            },
        )
        .unwrap();
        let copied = destination.join(source.file_name().unwrap());
        assert!(copied.join(nested.file_name().unwrap()).is_dir());
        assert_eq!(fs::read(copied.join("vuoto.txt")).unwrap(), b"");
        assert_eq!(fs::read(copied.join("contenuto.txt")).unwrap(), b"CONTENTS");
        assert_eq!(fs::read(source.join("contenuto.txt")).unwrap(), b"CONTENTS");
    }

    #[cfg(windows)]
    #[test]
    fn windows_real_256_directory_copy_fits_two_mib_stack_and_257_fails_before_batch_writes() {
        std::thread::Builder::new()
            .name("directory-depth-256".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                fn phase(message: &[u8]) {
                    std::io::stderr()
                        .write_all(message)
                        .expect("Depth-test phase marker");
                }
                let root = tempfile::tempdir().unwrap();
                let source = root.path().join("s");
                let destination = root.path().join("out");
                let rejected_destination = root.path().join("rejected");
                fs::create_dir(&source).unwrap();
                fs::create_dir(&destination).unwrap();
                fs::create_dir(&rejected_destination).unwrap();
                let mut deepest = source.clone();
                for _ in 1..256 {
                    deepest.push("d");
                    fs::create_dir(&deepest).unwrap();
                }
                let bytes = vec![0x7b; 300_000];
                fs::write(deepest.join("file.bin"), &bytes).unwrap();
                let active = std::rc::Rc::new(std::cell::Cell::new(0usize));
                let maximum = std::rc::Rc::new(std::cell::Cell::new(0usize));
                let observed_active = active.clone();
                let observed_max = maximum.clone();
                phase(b"VELDIR depth256: before copy\n");
                with_directory_copy_test_settings(
                    MAX_DIRECTORY_GUARDS,
                    move |stage, _| {
                        if stage == CopyDirectoryStage::Validated {
                            let count = observed_active.get() + 1;
                            observed_active.set(count);
                            observed_max.set(observed_max.get().max(count));
                        } else if stage == CopyDirectoryStage::BeforeFinal {
                            observed_active.set(observed_active.get() - 1);
                        }
                        Ok(())
                    },
                    || {
                        execute(Job::Copy {
                            source: source.clone(),
                            destination_folder: destination.clone(),
                        })
                    },
                )
                .unwrap();
                phase(b"VELDIR depth256: after copy\n");
                assert_eq!(maximum.get(), 256);
                assert_eq!(active.get(), 0);
                let copied = destination
                    .join("s")
                    .join(deepest.strip_prefix(&source).unwrap())
                    .join("file.bin");
                assert_eq!(fs::read(copied).unwrap(), bytes);
                assert_eq!(fs::read(deepest.join("file.bin")).unwrap(), bytes);
                phase(b"VELDIR depth256: after verification\n");
                fs::create_dir(deepest.join("too-deep")).unwrap();
                let first = root.path().join("first.txt");
                fs::write(&first, b"FIRST MUST NOT BE PUBLISHED").unwrap();
                phase(b"VELDIR depth256: before depth257 preflight\n");
                let error = execute(Job::CopyMany {
                    sources: vec![first.clone(), source],
                    destination_folder: rejected_destination.clone(),
                })
                .unwrap_err();
                phase(b"VELDIR depth256: after depth257 preflight\n");
                assert!(error.contains("massimo 256 directory"), "{error}");
                assert_eq!(fs::read_dir(rejected_destination).unwrap().count(), 0);
                assert_eq!(fs::read(first).unwrap(), b"FIRST MUST NOT BE PUBLISHED");
                phase(b"VELDIR depth256: before fixture cleanup\n");
                drop(root);
                phase(b"VELDIR depth256: after fixture cleanup\n");
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[cfg(windows)]
    fn assert_source_handle_allows_readers_and_denies_changes(source: &Path) {
        let reader = fs::File::open(source).expect("A concurrent reader must be allowed");
        assert!(reader.metadata().unwrap().is_file());
        drop(reader);
        let writer_error = fs::OpenOptions::new().write(true).open(source).unwrap_err();
        assert_eq!(
            writer_error.raw_os_error(),
            Some(32),
            "Expected ERROR_SHARING_VIOLATION: {writer_error}"
        );
        let renamed = source.with_extension("denied-rename");
        assert!(!renamed.exists());
        assert!(
            fs::rename(source, &renamed).is_err(),
            "Protected source must deny rename"
        );
        assert!(
            fs::remove_file(source).is_err(),
            "Protected source must deny deletion"
        );
        assert!(source.exists());
        assert!(!renamed.exists());
    }

    #[cfg(windows)]
    fn assert_source_handle_released(source: &Path) {
        let writer = fs::OpenOptions::new()
            .write(true)
            .open(source)
            .expect("Source writer must be allowed after handle release");
        drop(writer);
        let renamed = source.with_extension("released-rename");
        assert!(!renamed.exists());
        fs::rename(source, &renamed).expect("Source rename must be allowed after handle release");
        fs::rename(&renamed, source).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_replacement_before_source_open_creates_no_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let saved = root.path().join("saved-original.txt");
        let output = root.path().join("output.txt");
        fs::write(&source, b"KEEP ORIGINAL").unwrap();
        let original = source.clone();
        let archived = saved.clone();
        let validated = std::rc::Rc::new(std::cell::Cell::new(false));
        let hit = validated.clone();
        let result = with_copy_source_hook(
            move |stage, path| {
                assert_eq!(path, original);
                if stage == CopySourceStage::BeforeOpen {
                    fs::rename(path, &archived).unwrap();
                    fs::create_dir(path).unwrap();
                } else {
                    hit.set(true);
                }
                Ok(())
            },
            || {
                copy_tree_progress_with_sync(
                    &source,
                    &output,
                    &AtomicBool::new(false),
                    &mut |_| {},
                    &mut |file| file.sync_all(),
                )
            },
        );
        assert!(result.is_err());
        assert!(!validated.get());
        assert!(!output.exists());
        assert!(source.is_dir());
        assert_eq!(fs::read(saved).unwrap(), b"KEEP ORIGINAL");
    }

    #[cfg(windows)]
    #[test]
    fn windows_regular_replacement_before_open_defines_the_source_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let saved = root.path().join("saved-original.txt");
        let replacement = root.path().join("replacement.txt");
        let output = root.path().join("output.txt");
        fs::write(&source, b"ORIGINAL").unwrap();
        fs::write(&replacement, b"REPLACED").unwrap();
        let modified = fs::metadata(&source).unwrap().modified().unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            fs::metadata(&replacement).unwrap().len(),
            fs::metadata(&source).unwrap().len()
        );
        assert_eq!(
            fs::metadata(&replacement).unwrap().modified().unwrap(),
            modified
        );
        let archived = saved.clone();
        let incoming = replacement.clone();
        let output_path = output.clone();
        let result = with_copy_source_hook(
            move |stage, path| {
                if stage == CopySourceStage::BeforeOpen {
                    fs::rename(path, &archived).unwrap();
                    fs::rename(&incoming, path).unwrap();
                } else {
                    assert!(
                        !output_path.exists(),
                        "Handle validation must precede output creation"
                    );
                    assert_source_handle_allows_readers_and_denies_changes(path);
                }
                Ok(())
            },
            || {
                copy_tree_progress_with_sync(
                    &source,
                    &output,
                    &AtomicBool::new(false),
                    &mut |_| {},
                    &mut |file| file.sync_all(),
                )
            },
        );
        result.unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"REPLACED");
        assert_eq!(fs::read(output).unwrap(), b"REPLACED");
        assert_eq!(fs::read(saved).unwrap(), b"ORIGINAL");
        assert_source_handle_released(&source);
    }

    #[cfg(windows)]
    #[test]
    fn windows_existing_writer_prevents_protected_source_open_before_output() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let output = root.path().join("output.txt");
        fs::write(&source, b"KEEP").unwrap();
        // The default writer shares everything; the new source handle must
        // still refuse to coexist with its existing data-write access.
        let writer = fs::OpenOptions::new().write(true).open(&source).unwrap();
        let error = copy_tree_progress_with_sync(
            &source,
            &output,
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |file| file.sync_all(),
        )
        .unwrap_err();
        assert!(error.contains("Apertura sorgente protetta"), "{error}");
        assert!(!output.exists());
        drop(writer);
        assert_eq!(fs::read(&source).unwrap(), b"KEEP");
        assert_source_handle_released(&source);
    }

    #[cfg(windows)]
    #[test]
    fn windows_source_handle_remains_protected_during_chunks_and_destination_sync() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("sorgente résumé 漢字.bin");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let bytes = vec![0xa5; 800_000];
        fs::write(&source, &bytes).unwrap();
        let mut chunks = 0;
        let mut syncs = 0;
        copy_one_progress_with_sync(
            &source,
            &destination,
            &AtomicBool::new(false),
            &mut |_| {
                chunks += 1;
                assert_source_handle_allows_readers_and_denies_changes(&source);
            },
            &mut |staged| {
                syncs += 1;
                assert_source_handle_allows_readers_and_denies_changes(&source);
                staged.sync_all()
            },
        )
        .unwrap();
        assert!(chunks >= 2);
        assert_eq!(syncs, 1);
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(
            fs::read(destination.join(source.file_name().unwrap())).unwrap(),
            bytes
        );
        assert_source_handle_released(&source);
    }

    #[cfg(windows)]
    #[test]
    fn windows_source_handle_is_released_after_sync_failure_and_chunk_cancellation() {
        for sync_failure in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source.bin");
            let destination = root.path().join("destination");
            fs::create_dir(&destination).unwrap();
            let bytes = vec![0x3c; 600_000];
            fs::write(&source, &bytes).unwrap();
            let cancelled = AtomicBool::new(false);
            let mut syncs = 0;
            let error = copy_one_progress_with_sync(
                &source,
                &destination,
                &cancelled,
                &mut |_| {
                    assert_source_handle_allows_readers_and_denies_changes(&source);
                    if !sync_failure {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                },
                &mut |staged| {
                    syncs += 1;
                    assert_source_handle_allows_readers_and_denies_changes(&source);
                    if sync_failure {
                        Err(io::Error::from(io::ErrorKind::StorageFull))
                    } else {
                        staged.sync_all()
                    }
                },
            )
            .unwrap_err();
            assert_eq!(syncs, if sync_failure { 1 } else { 0 });
            if sync_failure {
                assert!(error.contains("Sincronizzazione copia"));
            } else {
                assert!(error.contains("annullata"));
            }
            assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
            assert_eq!(fs::read(&source).unwrap(), bytes);
            assert_source_handle_released(&source);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_cancel_after_source_handle_validation_creates_no_output_and_releases_handle() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let output = root.path().join("output.txt");
        fs::write(&source, b"KEEP").unwrap();
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let injected = cancelled.clone();
        let error = with_copy_source_hook(
            move |stage, path| {
                if stage == CopySourceStage::Validated {
                    assert_source_handle_allows_readers_and_denies_changes(path);
                    injected.store(true, Ordering::Relaxed);
                }
                Ok(())
            },
            || {
                copy_tree_progress_with_sync(
                    &source,
                    &output,
                    &cancelled,
                    &mut |_| {},
                    &mut |file| file.sync_all(),
                )
            },
        )
        .unwrap_err();
        assert!(error.contains("annullato"));
        assert!(!output.exists());
        assert_eq!(fs::read(&source).unwrap(), b"KEEP");
        assert_source_handle_released(&source);
    }

    #[cfg(windows)]
    #[test]
    fn windows_final_handle_timestamp_check_rejects_metadata_change_during_sync() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        };
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"KEEP CONTENTS").unwrap();
        let modified = fs::metadata(&source).unwrap().modified().unwrap();
        let changed = modified + Duration::from_secs(120);
        let error = copy_one_progress_with_sync(
            &source,
            &destination,
            &AtomicBool::new(false),
            &mut |_| {},
            &mut |staged| {
                // Sharing modes protect data access, not attribute-only access.
                // Exercise the final metadata check without bypassing the data lock.
                let attributes = fs::OpenOptions::new()
                    .access_mode(FILE_WRITE_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                    .open(&source)?;
                attributes.set_times(fs::FileTimes::new().set_modified(changed))?;
                drop(attributes);
                staged.sync_all()
            },
        )
        .unwrap_err();
        assert!(
            error.contains("Sorgente cambiata dopo l'apertura protetta"),
            "{error}"
        );
        assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), changed);
        assert_eq!(fs::read(&source).unwrap(), b"KEEP CONTENTS");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
        assert_source_handle_released(&source);
    }

    #[test]
    fn protected_source_copy_preserves_unicode_and_empty_file_contents() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        for (name, bytes) in [
            ("vuoto résumé 漢字.txt", &b""[..]),
            ("contenuto résumé 漢字.txt", &b"hello"[..]),
        ] {
            let source = root.path().join(name);
            fs::write(&source, bytes).unwrap();
            let validated = std::rc::Rc::new(std::cell::Cell::new(false));
            let hit = validated.clone();
            with_copy_source_hook(
                move |stage, path| {
                    if stage == CopySourceStage::Validated {
                        hit.set(true);
                        #[cfg(windows)]
                        assert_source_handle_allows_readers_and_denies_changes(path);
                        #[cfg(not(windows))]
                        assert!(path.is_file());
                    }
                    Ok(())
                },
                || copy_one_progress(&source, &destination, &AtomicBool::new(false), &mut |_| {}),
            )
            .unwrap();
            assert!(validated.get());
            assert_eq!(fs::read(&source).unwrap(), bytes);
            assert_eq!(fs::read(destination.join(name)).unwrap(), bytes);
            #[cfg(windows)]
            assert_source_handle_released(&source);
        }
        assert_eq!(fs::read_dir(destination).unwrap().count(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn windows_symlink_replacement_is_rejected_without_following_its_target() {
        use std::os::windows::fs::symlink_file;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let saved = root.path().join("saved-original.txt");
        let linked = root.path().join("linked.txt");
        let output = root.path().join("output.txt");
        fs::write(&source, b"KEEP ORIGINAL").unwrap();
        fs::write(&linked, b"KEEP LINK TARGET").unwrap();
        let archived = saved.clone();
        let reference = linked.clone();
        let result = with_copy_source_hook(
            move |stage, path| {
                if stage == CopySourceStage::BeforeOpen {
                    fs::rename(path, &archived).unwrap();
                    symlink_file(&reference, path).expect(
                        "Creazione symlink Windows fallita: il runner richiede CreateSymbolicLink o Developer Mode; prova non eseguita, nessuno skip automatico",
                    );
                } else {
                    panic!("A reparse point must not reach validated source state");
                }
                Ok(())
            },
            || {
                copy_tree_progress_with_sync(
                    &source,
                    &output,
                    &AtomicBool::new(false),
                    &mut |_| {},
                    &mut |file| file.sync_all(),
                )
            },
        );
        assert!(result.is_err());
        assert!(!output.exists());
        assert_eq!(fs::read(saved).unwrap(), b"KEEP ORIGINAL");
        assert_eq!(fs::read(linked).unwrap(), b"KEEP LINK TARGET");
        assert!(
            fs::symlink_metadata(source)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn execute_copy_obeys_entry_and_pending_budgets_before_writes() {
        for (max_entries, max_pending_bytes, expected) in [
            (2, 1024 * 1024, "massimo 2 elementi"),
            (100, 1, "limite memoria"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source");
            let destination = root.path().join("destination");
            fs::create_dir(&source).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(source.join("a.txt"), b"A").unwrap();
            fs::write(source.join("b.txt"), b"BB").unwrap();
            fs::write(destination.join("existing.txt"), b"KEEP").unwrap();
            let error = with_scan_test_settings(
                max_entries,
                max_pending_bytes,
                |_, _| Ok(()),
                || {
                    execute(Job::Copy {
                        source: source.clone(),
                        destination_folder: destination.clone(),
                    })
                },
            )
            .unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
            assert_eq!(fs::read(destination.join("existing.txt")).unwrap(), b"KEEP");
            assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"A");
            assert_eq!(fs::read(source.join("b.txt")).unwrap(), b"BB");
        }
    }

    #[test]
    fn execute_copy_many_missing_or_unreadable_second_source_prevents_first_publication() {
        for missing in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let destination = root.path().join("destination");
            let first = root.path().join("first.txt");
            let second = root.path().join("second");
            fs::create_dir(&destination).unwrap();
            fs::write(destination.join("existing.txt"), b"KEEP").unwrap();
            fs::write(&first, b"FIRST").unwrap();
            if !missing {
                fs::create_dir(&second).unwrap();
                fs::write(second.join("child.txt"), b"CHILD").unwrap();
            }
            let fault_path = second.clone();
            let injected = std::rc::Rc::new(std::cell::Cell::new(false));
            let hit = injected.clone();
            let error = with_scan_test_settings(
                100,
                1024 * 1024,
                move |operation, path| {
                    if !missing && operation == ScanOperation::Next && path == fault_path {
                        hit.set(true);
                        return Err("simulated directory enumeration failure".into());
                    }
                    Ok(())
                },
                || {
                    execute(Job::CopyMany {
                        sources: vec![first.clone(), second.clone()],
                        destination_folder: destination.clone(),
                    })
                },
            )
            .unwrap_err();
            if missing {
                assert!(error.contains("second"), "{error}");
                assert!(!injected.get());
            } else {
                assert!(
                    error.contains("simulated directory enumeration failure"),
                    "{error}"
                );
                assert!(injected.get());
                assert_eq!(fs::read(second.join("child.txt")).unwrap(), b"CHILD");
            }
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
            assert_eq!(fs::read(destination.join("existing.txt")).unwrap(), b"KEEP");
            assert_eq!(fs::read(first).unwrap(), b"FIRST");
        }
    }

    #[test]
    fn execute_copy_many_shares_budget_across_roots_and_scans_each_entry_once() {
        for max_entries in [3, 4] {
            let root = tempfile::tempdir().unwrap();
            let first = root.path().join("first");
            let second = root.path().join("second");
            let destination = root.path().join("destination");
            fs::create_dir(&first).unwrap();
            fs::create_dir(&second).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(first.join("a.txt"), b"A").unwrap();
            fs::write(second.join("b.txt"), b"BB").unwrap();
            let visits = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let recorded = visits.clone();
            let result = with_scan_test_settings(
                max_entries,
                1024 * 1024,
                move |operation, path| {
                    if operation == ScanOperation::Metadata {
                        recorded.borrow_mut().push(path.to_path_buf());
                    }
                    Ok(())
                },
                || {
                    execute(Job::CopyMany {
                        sources: vec![first.clone(), second.clone()],
                        destination_folder: destination.clone(),
                    })
                },
            );
            if max_entries == 3 {
                assert!(result.unwrap_err().contains("massimo 3 elementi"));
                assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
            } else {
                assert_eq!(result.unwrap(), "Operazione completata su 2 elementi");
                assert_eq!(
                    *visits.borrow(),
                    vec![
                        first.clone(),
                        first.join("a.txt"),
                        second.clone(),
                        second.join("b.txt")
                    ]
                );
                assert_eq!(
                    fs::read(destination.join("first").join("a.txt")).unwrap(),
                    b"A"
                );
                assert_eq!(
                    fs::read(destination.join("second").join("b.txt")).unwrap(),
                    b"BB"
                );
                assert_eq!(fs::read_dir(&destination).unwrap().count(), 2);
            }
            assert_eq!(fs::read(first.join("a.txt")).unwrap(), b"A");
            assert_eq!(fs::read(second.join("b.txt")).unwrap(), b"BB");
        }
    }

    #[test]
    fn execute_copy_scans_once_and_preserves_success_message() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"CONTENTS").unwrap();
        let visits = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = visits.clone();
        let result = with_scan_test_settings(
            1,
            1024 * 1024,
            move |operation, path| {
                if operation == ScanOperation::Metadata {
                    recorded.borrow_mut().push(path.to_path_buf());
                }
                Ok(())
            },
            || {
                execute(Job::Copy {
                    source: source.clone(),
                    destination_folder: destination.clone(),
                })
            },
        )
        .unwrap();
        assert_eq!(result, format!("Copiato in {}", destination.display()));
        assert_eq!(*visits.borrow(), vec![source.clone()]);
        assert_eq!(fs::read(source).unwrap(), b"CONTENTS");
        assert_eq!(
            fs::read(destination.join("source.txt")).unwrap(),
            b"CONTENTS"
        );
        assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
    }

    #[test]
    fn execute_copy_entrypoints_preserve_preexisting_and_new_conflicts() {
        for batch in [false, true] {
            for preexisting in [true, false] {
                let root = tempfile::tempdir().unwrap();
                let first = root.path().join("first.txt");
                let second = root.path().join("second.txt");
                let destination = root.path().join("destination");
                fs::create_dir(&destination).unwrap();
                fs::write(&first, b"FIRST").unwrap();
                fs::write(&second, b"SECOND").unwrap();
                let conflict = destination.join("second.txt");
                if preexisting {
                    fs::write(&conflict, b"KEEP CONFLICT").unwrap();
                }
                let collision_source = second.clone();
                let collision_target = conflict.clone();
                let result = with_scan_test_settings(
                    100,
                    1024 * 1024,
                    move |operation, path| {
                        if !preexisting
                            && operation == ScanOperation::Metadata
                            && path == collision_source
                        {
                            fs::write(&collision_target, b"KEEP CONFLICT").unwrap();
                        }
                        Ok(())
                    },
                    || {
                        if batch {
                            execute(Job::CopyMany {
                                sources: vec![first.clone(), second.clone()],
                                destination_folder: destination.clone(),
                            })
                        } else {
                            execute(Job::Copy {
                                source: second.clone(),
                                destination_folder: destination.clone(),
                            })
                        }
                    },
                );
                assert!(result.is_err());
                assert_eq!(fs::read(&first).unwrap(), b"FIRST");
                assert_eq!(fs::read(&second).unwrap(), b"SECOND");
                assert_eq!(fs::read(&conflict).unwrap(), b"KEEP CONFLICT");
                // A conflict introduced after preflight can follow a published
                // first item; existing conflicts prevent the entire batch.
                let first_published = batch && !preexisting;
                assert_eq!(destination.join("first.txt").exists(), first_published);
                assert_eq!(
                    fs::read_dir(&destination).unwrap().count(),
                    if first_published { 2 } else { 1 }
                );
                if first_published {
                    assert_eq!(fs::read(destination.join("first.txt")).unwrap(), b"FIRST");
                }
            }
        }
    }

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
