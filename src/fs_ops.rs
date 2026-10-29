use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn list(path: &Path) -> Result<Vec<Entry>, String> {
    let it = fs::read_dir(path)
        .map_err(|e| format!("Impossibile leggere {}: {e}", path.display()))?;
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
            size: if metadata.is_file() { metadata.len() } else { 0 },
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
    NewFolder { parent: PathBuf, name: String },
    Rename { source: PathBuf, name: String },
    Copy { source: PathBuf, destination_folder: PathBuf },
    Move { source: PathBuf, destination_folder: PathBuf },
    Trash { source: PathBuf },
    CopyMany { sources: Vec<PathBuf>, destination_folder: PathBuf },
    MoveMany { sources: Vec<PathBuf>, destination_folder: PathBuf },
    TrashMany { sources: Vec<PathBuf> },
    RenameMany { changes: Vec<(PathBuf, String)> },
}

pub fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." || name.trim() != name {
        return Err("Inserisci un nome valido, senza spazi iniziali/finali".into());
    }
    if name.ends_with('.') || name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return Err("Il nome contiene caratteri non consentiti su Windows".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
    {
        return Err("Nome riservato di Windows".into());
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
    if target.symlink_metadata().is_ok() {
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
    let meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() || is_windows_reparse_point(&meta) {
        return Err(format!(
            "Collegamenti e punti di reindirizzamento non copiati: {}",
            source.display()
        ));
    }
    if meta.is_dir() {
        fs::create_dir(target).map_err(|e| e.to_string())?;
        for child in fs::read_dir(source).map_err(|e| e.to_string())? {
            let child = child.map_err(|e| e.to_string())?;
            copy_no_links(&child.path(), &target.join(child.file_name()))?;
        }
    } else if meta.is_file() {
        let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
    } else {
        return Err("Tipo di file non supportato".into());
    }
    Ok(())
}

/// Atomic rename without replacing a destination: don't use std::fs::rename!
/// On Windows its replacement behavior depends on the filesystem and OS.
#[cfg(windows)]
fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let mut src: Vec<u16> = source.as_os_str().encode_wide().collect();
    let mut dst: Vec<u16> = target.as_os_str().encode_wide().collect();
    if src.contains(&0) || dst.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Percorso non valido"));
    }
    src.push(0);
    dst.push(0);
    // Zero flags: in particular, do NOT pass MOVEFILE_REPLACE_EXISTING.
    let result = unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) };
    if result == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Linux helper for development/testing; Windows is the distribution target.
#[cfg(target_os = "linux")]
fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let src = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Percorso non valido"))?;
    let dst = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Percorso non valido"))?;
    let result = unsafe {
        libc::renameat2(libc::AT_FDCWD, src.as_ptr(), libc::AT_FDCWD, dst.as_ptr(), libc::RENAME_NOREPLACE)
    };
    if result == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn rename_no_replace(_source: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Piattaforma non supportata"))
}

/// Preflight every source before a batch starts. This cannot rule out concurrent
/// filesystem changes: a failed batch can therefore be PARTIALLY completed.
fn preflight_batch(sources: &[PathBuf], destination_folder: &Path) -> Result<(), String> {
    if sources.is_empty() { return Err("Nessun elemento selezionato".into()); }
    let mut names = BTreeSet::new();
    for source in sources {
        // Reject duplicate destination names, even if case differs (Windows semantics).
        let name = source.file_name().ok_or("Elemento senza nome")?
            .to_string_lossy().to_lowercase();
        if !names.insert(name) { return Err("Due sorgenti hanno lo stesso nome".into()); }
        if source.symlink_metadata().is_err() {
            return Err(format!("Sorgente non disponibile: {}", source.display()));
        }
        target_path(source, destination_folder)?;
    }
    Ok(())
}

/// Preview a numbered bulk-rename plan; extension is preserved for files.
pub fn rename_plan(sources: &[PathBuf], prefix: &str, find: &str, replace: &str, start: u32)
    -> Result<Vec<(PathBuf, String)>, String> {
    if sources.is_empty() { return Err("Seleziona almeno un elemento".into()); }
    if sources.len() > 10_000 { return Err("Massimo 10.000 nomi per operazione".into()); }
    let mut result = Vec::with_capacity(sources.len());
    for (i, source) in sources.iter().enumerate() {
        let filename = source.file_name().and_then(|s| s.to_str())
            .ok_or("Nome file non Unicode: operazione non supportata")?;
        let extension = if source.is_file() { source.extension().and_then(|s| s.to_str()) } else { None };
        let stem = if extension.is_some() { source.file_stem().and_then(|s| s.to_str()).unwrap_or(filename) } else { filename };
        let replaced = if find.is_empty() { stem.to_owned() } else { stem.replace(find, replace) };
        let number = start.checked_add(i as u32).ok_or("Contatore troppo grande")?;
        let mut new_name = format!("{prefix}{number:03}_{replaced}");
        if let Some(ext) = extension { new_name.push('.'); new_name.push_str(ext); }
        valid_name(&new_name)?;
        result.push((source.clone(), new_name));
    }
    Ok(result)
}

fn preflight_rename_many(changes: &[(PathBuf, String)]) -> Result<(), String> {
    if changes.is_empty() { return Err("Nessun elemento da rinominare".into()); }
    let mut targets = BTreeSet::new();
    for (source, name) in changes {
        valid_name(name)?;
        if source.symlink_metadata().is_err() { return Err(format!("Sorgente assente: {}", source.display())); }
        let parent = source.parent().ok_or("Percorso senza cartella padre")?;
        let target = parent.join(name);
        if target == *source { return Err("Un nome rimane invariato: modifica i parametri".into()); }
        if !targets.insert(target.to_string_lossy().to_lowercase()) {
            return Err("Due file avrebbero lo stesso nome: operazione interrotta".into());
        }
        if target.symlink_metadata().is_ok() { return Err(format!("Destinazione già presente: {}", target.display())); }
    }
    Ok(())
}

/// Progress is per item. Cancellation is safe between items; an in-progress file finishes first.
pub fn execute_progress<F: Fn(usize, usize)>(
    job: Job, cancelled: &AtomicBool, notify: F
) -> Result<String, String> {
    match job {
        Job::CopyMany { sources, destination_folder } => {
            preflight_batch(&sources, &destination_folder)?;
            execute_batch_progress(sources, cancelled, notify,
                |source| Job::Copy { source, destination_folder: destination_folder.clone() })
        }
        Job::MoveMany { sources, destination_folder } => {
            preflight_batch(&sources, &destination_folder)?;
            execute_batch_progress(sources, cancelled, notify,
                |source| Job::Move { source, destination_folder: destination_folder.clone() })
        }
        Job::TrashMany { sources } => execute_batch_progress(sources, cancelled, notify,
            |source| Job::Trash { source }),
        Job::RenameMany { changes } => {
            preflight_rename_many(&changes)?;
            let total = changes.len();
            for (idx, (source,name)) in changes.into_iter().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(format!("Annullato dopo {idx}/{total} elementi; modifiche precedenti conservate"));
                }
                execute(Job::Rename {source,name})?;
                notify(idx+1,total);
            }
            Ok(format!("Rinominati {total} elementi"))
        }
        single => execute(single),
    }
}

fn execute_batch_progress<F: Fn(usize,usize)>(
    sources: Vec<PathBuf>, cancelled: &AtomicBool, notify: F,
    action: impl Fn(PathBuf) -> Job
) -> Result<String,String> {
    if sources.is_empty() { return Err("Nessun elemento selezionato".into()); }
    let total=sources.len();
    for (idx,source) in sources.into_iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(format!("Annullato dopo {idx}/{total} elementi; modifiche precedenti conservate"));
        }
        execute(action(source)).map_err(|e|
            format!("Interrotto dopo {idx}/{total} elementi, alcuni già modificati: {e}"))?;
        notify(idx+1,total);
    }
    Ok(format!("Completati {total} elementi"))
}

fn run_batch(sources: Vec<PathBuf>, action: impl Fn(PathBuf) -> Job) -> Result<String, String> {
    if sources.is_empty() { return Err("Nessun elemento selezionato".into()); }
    let total = sources.len();
    for (i, source) in sources.into_iter().enumerate() {
        if let Err(error) = execute(action(source)) {
            return Err(format!("Operazione interrotta dopo {i}/{total} elementi. Alcune modifiche potrebbero essere già state effettuate. Dettaglio: {error}"));
        }
    }
    Ok(format!("Operazione completata su {total} elementi"))
}

pub fn execute(job: Job) -> Result<String, String> {
    match job {
        Job::NewFolder { parent, name } => {
            valid_name(&name)?;
            fs::create_dir(parent.join(&name)).map_err(|e| e.to_string())?;
            Ok(format!("Cartella '{name}' creata"))
        }
        Job::Rename { source, name } => {
            valid_name(&name)?;
            let parent = source.parent().ok_or("Elemento senza cartella padre")?;
            let target = parent.join(&name);
            if target == source { return Ok("Nome invariato".into()); }
            rename_no_replace(&source, &target)
                .map_err(|e| format!("Rinomina non riuscita (nessuna sovrascrittura): {e}"))?;
            Ok(format!("Rinominato in '{name}'"))
        }
        Job::Copy { source, destination_folder } => {
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
        Job::Move { source, destination_folder } => {
            let target = target_path(&source, &destination_folder)?;
            rename_no_replace(&source, &target).map_err(|e| {
                format!("Spostamento non riuscito: {e}. Tra dischi differenti usa Copia e poi Cestino")
            })?;
            Ok(format!("Spostato in {}", destination_folder.display()))
        }
        Job::Trash { source } => {
            trash::delete(&source).map_err(|e| format!("Impossibile spostare nel Cestino: {e}"))?;
            Ok("Elemento spostato nel Cestino".into())
        }
        Job::CopyMany { sources, destination_folder } => {
            preflight_batch(&sources, &destination_folder)?;
            run_batch(sources, |source| Job::Copy { source, destination_folder: destination_folder.clone() })
        }
        Job::MoveMany { sources, destination_folder } => {
            preflight_batch(&sources, &destination_folder)?;
            run_batch(sources, |source| Job::Move { source, destination_folder: destination_folder.clone() })
        }
        Job::TrashMany { sources } => {
            run_batch(sources, |source| Job::Trash { source })
        }
        Job::RenameMany { changes } => {
            preflight_rename_many(&changes)?;
            let total = changes.len();
            for (idx,(source,name)) in changes.into_iter().enumerate() {
                execute(Job::Rename {source,name}).map_err(|e|
                    format!("Rinomina interrotta dopo {idx}/{total}: {e}"))?;
            }
            Ok(format!("Rinominati {total} elementi"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_rename_preflight_never_overwrites() {
        let dir=tempfile::tempdir().unwrap();
        let a=dir.path().join("a.txt");let b=dir.path().join("b.txt");
        fs::write(&a,"AAA").unwrap();fs::write(&b,"BBB").unwrap();
        let plan=rename_plan(&[a.clone(),b.clone()],"pic_","","",1).unwrap();
        assert_eq!(plan[0].1,"pic_001_a.txt");
        assert_eq!(plan[1].1,"pic_002_b.txt");
        fs::write(dir.path().join("pic_002_b.txt"),"ORIGINAL").unwrap();
        assert!(execute(Job::RenameMany{changes:plan}).is_err());
        assert_eq!(fs::read_to_string(a).unwrap(),"AAA");
        assert_eq!(fs::read_to_string(b).unwrap(),"BBB");
    }
    #[test]
    fn cancellation_keeps_prior_items_and_leaves_next_untouched() {
        let dir=tempfile::tempdir().unwrap();
        let from=dir.path().join("from");let to=dir.path().join("to");
        fs::create_dir(&from).unwrap(); fs::create_dir(&to).unwrap();
        let a=from.join("a.txt");let b=from.join("b.txt");
        fs::write(&a,"a").unwrap();fs::write(&b,"b").unwrap();
        let cancelled=AtomicBool::new(false);
        let result=execute_progress(Job::CopyMany {
            sources:vec![a,b],destination_folder:to.clone()
        },&cancelled,|done,_|{ if done==1 {cancelled.store(true,Ordering::Relaxed);} });
        assert!(result.is_err());
        assert!(to.join("a.txt").exists());
        assert!(!to.join("b.txt").exists());
    }
    #[test]
    fn windows_bad_names_rejected() {
        for name in ["", "..", "foo/bar", "CON.txt", "LPT1", "test.", "a\\b", "hi "] {
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
        let err = execute(Job::Copy { source: source.clone(), destination_folder: temp.path().to_path_buf() });
        assert!(err.is_err());
        assert_eq!(fs::read_to_string(&source).unwrap(), "originale");
    }

    #[test]
    fn create_rename_copy_move_without_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        execute(Job::NewFolder { parent: temp.path().to_path_buf(), name: "Nuova".into() }).unwrap();
        execute(Job::Rename { source: temp.path().join("Nuova"), name: "Rinominata".into() }).unwrap();
        let dest = temp.path().join("Dest");
        fs::create_dir(&dest).unwrap();
        execute(Job::Copy { source: temp.path().join("Rinominata"), destination_folder: dest.clone() }).unwrap();
        assert!(dest.join("Rinominata").is_dir());
        fs::write(temp.path().join("report.txt"), "abc").unwrap();
        execute(Job::Move { source: temp.path().join("report.txt"), destination_folder: dest.clone() }).unwrap();
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
        execute(Job::Copy { source: from.join("text.txt"), destination_folder: to.clone() }).unwrap();
        assert_eq!(fs::read_to_string(to.join("text.txt")).unwrap(), "contenuto 🌟");
        let elements: Vec<_> = fs::read_dir(&to).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(elements.len(), 1, "Nessun residuo temporaneo previsto");
    }

    #[test]
    fn reject_copying_folder_into_descendant() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("parent");
        let child = parent.join("child");
        fs::create_dir_all(&child).unwrap();
        let result = execute(Job::Copy { source: parent, destination_folder: child.clone() });
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
        let result = execute(Job::Copy { source: from, destination_folder: to.clone() });
        assert!(result.is_err());
        assert_eq!(fs::read_dir(to).unwrap().count(), 0, "Un errore non deve pubblicare copie parziali");
    }

    #[test]
    fn rename_no_replace_prevents_existing_file_loss() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.txt");
        fs::write(&a, "A").unwrap();
        fs::write(&b, "B").unwrap();
        assert!(execute(Job::Rename { source: a.clone(), name: "b.txt".into() }).is_err());
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
        assert_eq!(fs::read_to_string(dest.join("second.txt")).unwrap(), "original");
    }

    #[test]
    fn batch_copy_moves_multiple_files_without_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        let to = tmp.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        for name in ["a.txt", "b.txt"] { fs::write(from.join(name), name).unwrap(); }
        execute(Job::CopyMany {
            sources: vec![from.join("a.txt"), from.join("b.txt")],
            destination_folder: to.clone(),
        }).unwrap();
        assert_eq!(fs::read_to_string(to.join("a.txt")).unwrap(), "a.txt");
        assert_eq!(fs::read_to_string(to.join("b.txt")).unwrap(), "b.txt");
    }

}
