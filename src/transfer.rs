use crate::fs_ops::{self, Job, Progress};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Stop,
    Skip,
    KeepBoth,
}
/// Internal error preserves whether a verified copy has already been
/// published, so transfer reports can distinguish partial success from failure.
#[derive(Debug)]
struct MoveFailure {
    message: String,
    published: bool,
}
impl MoveFailure {
    fn before(message: impl Into<String>) -> Self {
        Self { message: message.into(), published: false }
    }
    fn after(message: impl Into<String>) -> Self {
        Self { message: message.into(), published: true }
    }
    #[cfg(test)]
    fn contains(&self, needle: &str) -> bool {
        self.message.contains(needle)
    }
}
impl From<String> for MoveFailure {
    fn from(value: String) -> Self { Self::before(value) }
}
impl From<&str> for MoveFailure {
    fn from(value: &str) -> Self { Self::before(value) }
}
impl std::fmt::Display for MoveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
/// Only treat the OS-level "different volume" result as authorization
/// to copy-verify-delete. Permissions, locks and all other rename failures
/// must NOT be silently downgraded to a copy.
fn is_cross_device(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
}

/// Compare every byte without loading the files into memory. Run on the
/// transfer worker and check cancellation between reads. The source's
/// metadata must still match its pre-copy snapshot.
fn verify_copy(
    source: &Path,
    copy: &Path,
    snapshot: &fs::Metadata,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if !unchanged_source(source, snapshot) {
        return Err("Sorgente modificata durante la copia; originale conservato".into());
    }
    let mut original = fs::File::open(source).map_err(|e| format!("Riapertura sorgente: {e}"))?;
    let mut replica = fs::File::open(copy).map_err(|e| format!("Riapertura copia: {e}"))?;
    let mut source_buffer = [0u8; 256 * 1024];
    let mut copy_buffer = [0u8; 256 * 1024];
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Annullato prima della pubblicazione; originale conservato".into());
        }
        let n = original
            .read(&mut source_buffer)
            .map_err(|e| format!("Verifica sorgente: {e}"))?;
        if n == 0 {
            if replica
                .read(&mut copy_buffer[..1])
                .map_err(|e| format!("Verifica destinazione: {e}"))?
                != 0
            {
                return Err("Copia più lunga della sorgente; originale conservato".into());
            }
            break;
        }
        replica
            .read_exact(&mut copy_buffer[..n])
            .map_err(|e| format!("Copia incompleta: {e}"))?;
        if source_buffer[..n] != copy_buffer[..n] {
            return Err("Verifica byte fallita: la copia non coincide con la sorgente".into());
        }
    }
    if !unchanged_source(source, snapshot) {
        return Err("Sorgente modificata durante la verifica; originale conservato".into());
    }
    Ok(())
}

fn unchanged_source(source: &Path, snapshot: &fs::Metadata) -> bool {
    let Ok(now) = fs::symlink_metadata(source) else {
        return false;
    };
    now.is_file()
        && now.len() == snapshot.len()
        && snapshot
            .modified()
            .ok()
            .zip(now.modified().ok())
            .is_some_and(|(before, after)| before == after)
}

/// A manifest records names, file sizes and source timestamps before any
/// copy starts. Linking/reparse paths are never traversed, including if they
/// appear after the initial tree-size preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeStamp {
    is_directory: bool,
    bytes: u64,
    modified: std::time::SystemTime,
}

#[cfg(windows)]
fn reparse_point(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn reparse_point(_meta: &fs::Metadata) -> bool {
    false
}

fn snapshot_tree(root: &Path) -> Result<std::collections::BTreeMap<PathBuf, TreeStamp>, String> {
    let mut result = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if result.len() >= 100_000 {
            return Err("Cartella troppo grande: massimo 100.000 elementi".into());
        }
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Impossibile leggere {}: {e}", path.display()))?;
        if meta.file_type().is_symlink() || reparse_point(&meta) {
            return Err(format!(
                "Collegamento o junction non supportato: {}",
                path.display()
            ));
        }
        if !meta.is_dir() && !meta.is_file() {
            return Err(format!("Tipo non supportato: {}", path.display()));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_path_buf();
        let modified = meta
            .modified()
            .map_err(|e| format!("Data modifica non disponibile per {}: {e}", path.display()))?;
        result.insert(
            relative,
            TreeStamp {
                is_directory: meta.is_dir(),
                bytes: if meta.is_file() { meta.len() } else { 0 },
                modified,
            },
        );
        if meta.is_dir() {
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                pending.push(entry.map_err(|e| e.to_string())?.path());
            }
        }
    }
    Ok(result)
}

fn verify_folder_tree(
    source: &Path,
    replica: &Path,
    expected: &std::collections::BTreeMap<PathBuf, TreeStamp>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let current_source = snapshot_tree(source)?;
    if &current_source != expected {
        return Err(
            "La cartella sorgente è cambiata durante la copia; originale conservato".into(),
        );
    }
    let current_replica = snapshot_tree(replica)?;
    if current_replica.len() != expected.len() {
        return Err("Copia cartella incompleta: numero di elementi diverso".into());
    }
    for (relative, original) in expected {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Verifica cartella annullata; originale conservato".into());
        }
        let Some(copied) = current_replica.get(relative) else {
            return Err(format!(
                "Elemento mancante nella copia: {}",
                relative.display()
            ));
        };
        if original.is_directory != copied.is_directory || original.bytes != copied.bytes {
            return Err(format!(
                "Elemento diverso nella copia: {}",
                relative.display()
            ));
        }
        if !original.is_directory {
            let path = source.join(relative);
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            verify_copy(&path, &replica.join(relative), &meta, cancelled)?;
        }
    }
    if &snapshot_tree(source)? != expected {
        return Err("Sorgente modificata durante la verifica; originale conservato".into());
    }
    Ok(())
}

/// After successful verification, publish the new folder, then move the old
/// folder to the Windows Recycle Bin. This avoids partial recursive deletion.
/// A recycle error is an explicit partial success: both locations are reported.
/// The callback allows non-destructive tests without touching the OS bin.
fn move_folder_across_volumes(
    source: &Path,
    target: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    notify: &dyn Fn(Progress),
    recycle: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), MoveFailure> {
    let snapshot = snapshot_tree(source)?;
    if !snapshot.get(Path::new("")).is_some_and(|s| s.is_directory) {
        return Err("Sorgente non è una cartella".into());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Spostamento cartella annullato, sorgente conservata".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".veldir-cross-volume-folder-")
        .tempdir_in(destination)
        .map_err(|e| format!("Area temporanea non disponibile: {e}"))?;
    fs_ops::execute_progress(
        Job::CopyMany {
            sources: vec![source.to_path_buf()],
            destination_folder: staging.path().to_path_buf(),
        },
        cancelled,
        notify,
    )?;
    let name = source
        .file_name()
        .ok_or("Nome cartella sorgente non valido")?;
    let staged_folder = staging.path().join(name);
    verify_folder_tree(source, &staged_folder, &snapshot, cancelled)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Annullato prima della pubblicazione; sorgente conservata".into());
    }
    fs_ops::rename_no_replace(&staged_folder, target)
        .map_err(|e| format!("Impossibile pubblicare cartella; sorgente conservata: {e}"))?;
    if cancelled.load(Ordering::Relaxed) || snapshot_tree(source).ok().as_ref() != Some(&snapshot) {
        return Err(MoveFailure::after(format!(
            "Copia pubblicata in {}, ma sorgente {} conservata (annullamento o modifiche esterne)",
            target.display(),
            source.display()
        )));
    }
    recycle(source).map_err(|e| {
        MoveFailure::after(format!(
            "Cartella copiata e verificata in {}, ma impossibile spostare l'originale {} nel Cestino: {e}. Verifica entrambe le posizioni",
            target.display(),
            source.display()
        ))
    })
}

/// One file, staged on the destination volume and verified byte-for-byte.
/// Folder moves between volumes use a separate manifest-and-Recycle-Bin path.
fn move_one_with_rename(
    source: &Path,
    target: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    notify: &dyn Fn(Progress),
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), MoveFailure> {
    match rename(source, target) {
        Ok(()) => return Ok(()),
        Err(error) if !is_cross_device(&error) => {
            return Err(MoveFailure::before(format!("Spostamento non riuscito: {error}")));
        }
        Err(_) => {}
    }
    let snapshot =
        fs::symlink_metadata(source).map_err(|e| format!("Sorgente non leggibile: {e}"))?;
    // Preflight links/junctions and unsupported entries before copying.
    fs_ops::tree_size(source)?;
    if snapshot.is_dir() {
        return move_folder_across_volumes(
            source,
            target,
            destination,
            cancelled,
            notify,
            |folder| trash::delete(folder).map_err(|e| e.to_string()),
        );
    }
    if !snapshot.is_file() {
        return Err("Tipo di sorgente non supportato".into());
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Operazione annullata; sorgente non modificata".into());
    }
    // CopyMany uses its own temporary directory before publishing into our
    // private staging. All intermediate files are removed by RAII on failure.
    let staging = tempfile::Builder::new()
        .prefix(".veldir-cross-volume-")
        .tempdir_in(destination)
        .map_err(|e| format!("Impossibile preparare il volume di destinazione: {e}"))?;
    fs_ops::execute_progress(
        Job::CopyMany {
            sources: vec![source.to_path_buf()],
            destination_folder: staging.path().to_path_buf(),
        },
        cancelled,
        notify,
    )?;
    let name = source.file_name().ok_or("Nome sorgente non valido")?;
    let staged_file = staging.path().join(name);
    verify_copy(source, &staged_file, &snapshot, cancelled)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Annullato prima della pubblicazione; sorgente conservata".into());
    }
    fs_ops::rename_no_replace(&staged_file, target)
        .map_err(|e| format!("Impossibile pubblicare la copia, sorgente conservata: {e}"))?;
    // Once published, never delete the destination on cleanup failure: it is
    // a verified full copy. Report both paths so the user can reconcile them.
    if cancelled.load(Ordering::Relaxed) || !unchanged_source(source, &snapshot) {
        return Err(MoveFailure::after(format!(
            "Copia completa pubblicata in {}, ma sorgente {} conservata (annullamento o modifica esterna)",
            target.display(),
            source.display()
        )));
    }
    fs::remove_file(source).map_err(|e| {
        MoveFailure::after(format!(
            "Copia verificata in {}, ma impossibile eliminare la sorgente {}: {e}. Entrambe le copie possono essere presenti",
            target.display(),
            source.display()
        ))
    })
}

pub fn execute(
    sources: Vec<PathBuf>,
    destination: PathBuf,
    cut: bool,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    notify: impl Fn(Progress),
) -> Result<String, String> {
    if !destination.is_dir() {
        return Err("Destinazione non accessibile".into());
    }
    if sources.is_empty() {
        return Err("Nessuna sorgente selezionata".into());
    }
    let total = sources.len();
    // Moving on one volume is instant. When Windows falls back to copying
    // between volumes the byte progress must still have a real denominator.
    let total_bytes = sources
        .iter()
        .filter(|s| {
            policy != ConflictPolicy::Skip
                || s.file_name()
                    .is_some_and(|n| destination.join(n).symlink_metadata().is_err())
        })
        .filter_map(|s| fs_ops::tree_size(s).ok())
        .fold(0u64, |sum, n| sum.saturating_add(n));
    let mut completed_bytes = 0u64;
    let mut done = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();
    let mut partials = Vec::new();
    let mut details = Vec::new();
    // Stop policy validates all conflicts before publishing the first file.
    if policy == ConflictPolicy::Stop {
        let mut planned = std::collections::HashSet::new();
        for source in &sources {
            let name = source.file_name().ok_or("Nome sorgente non valido")?;
            if !planned.insert(name.to_os_string()) {
                return Err(format!(
                    "Conflitto nel batch: {}. Nessun elemento trasferito.",
                    name.to_string_lossy()
                ));
            }
            if destination.join(name).symlink_metadata().is_ok() {
                return Err(format!(
                    "Conflitto: {}. Nessun elemento trasferito.",
                    name.to_string_lossy()
                ));
            }
        }
    }
    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            details.push("Operazione annullata; gli elementi completati sono conservati.".into());
            break;
        }
        let Some(name) = source.file_name() else {
            errors.push(format!("Sorgente non valida: {}", source.display()));
            continue;
        };
        let original = destination.join(name);
        let conflict = original.symlink_metadata().is_ok();
        if conflict && policy == ConflictPolicy::Skip {
            skipped += 1;
            details.push(format!("Saltato: {}", source.display()));
            continue;
        }
        let target = if conflict {
            match unique_target(&destination, name.to_string_lossy().as_ref()) {
                Ok(path) => path,
                Err(error) => {
                    errors.push(format!("{}: {error}", source.display()));
                    if policy == ConflictPolicy::Stop {
                        break;
                    }
                    continue;
                }
            }
        } else {
            original
        };
        let result = if cut {
            let progress = |p: Progress| {
                notify(Progress {
                    done_items: done + skipped + errors.len() + partials.len(),
                    total_items: total,
                    done_bytes: completed_bytes.saturating_add(p.done_bytes),
                    total_bytes,
                })
            };
            move_one_with_rename(
                &source,
                &target,
                &destination,
                cancelled,
                &progress,
                fs_ops::rename_no_replace,
            )
        } else {
            // Copy into a private sibling, then atomically publish without replacement.
            let staging = tempfile::Builder::new()
                .prefix(".veldir-transfer-")
                .tempdir_in(&destination)
                .map_err(|e| e.to_string())?;
            let progress = |p: Progress| {
                notify(Progress {
                    done_items: done + skipped + errors.len() + partials.len(),
                    total_items: total,
                    done_bytes: completed_bytes.saturating_add(p.done_bytes),
                    total_bytes,
                })
            };
            fs_ops::execute_progress(
                Job::CopyMany {
                    sources: vec![source.clone()],
                    destination_folder: staging.path().to_path_buf(),
                },
                cancelled,
                &progress as &dyn Fn(Progress),
            )
            .and_then(|_| {
                if cancelled.load(Ordering::Relaxed) {
                    Err("Annullato prima della pubblicazione".into())
                } else {
                    fs_ops::rename_no_replace(&staging.path().join(name), &target)
                        .map_err(|e| e.to_string())
                }
            })
            .map_err(MoveFailure::before)
        };
        match result {
            Ok(_) => {
                completed_bytes =
                    completed_bytes.saturating_add(fs_ops::tree_size(&target).unwrap_or(0));
                done += 1;
                details.push(format!(
                    "Completato: {} → {}",
                    source.display(),
                    target.display()
                ));
            }
            Err(error) => {
                if error.published {
                    partials.push(format!(
                        "PARZIALE: {} → {}: {error}",
                        source.display(),
                        target.display()
                    ));
                } else {
                    errors.push(format!("{}: {error}", source.display()));
                }
                if policy == ConflictPolicy::Stop {
                    break;
                }
            }
        }
        notify(Progress {
            done_items: done + skipped + errors.len() + partials.len(),
            total_items: total,
            done_bytes: completed_bytes,
            total_bytes,
        });
    }
    let remaining = total.saturating_sub(done + skipped + partials.len() + errors.len());
    let summary = format!(
        "Completati {done}/{total} · parziali {} · saltati {skipped} · falliti {} · non elaborati {remaining}\n{}\n{}\n{}",
        partials.len(),
        errors.len(),
        details.join("\n"),
        partials.join("\n"),
        errors.join("\n")
    );
    if errors.is_empty()
        && partials.is_empty()
        && remaining == 0
        && !cancelled.load(Ordering::Relaxed)
    {
        Ok(summary)
    } else {
        Err(summary)
    }
}
fn unique_target(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let p = Path::new(name);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..=10_000 {
        let candidate = parent.join(format!("{stem} ({i}){ext}"));
        if candidate.symlink_metadata().is_err() {
            return Ok(candidate);
        }
    }
    Err("Troppi nomi in conflitto".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn windows_not_same_device_error_is_recognized() {
        // ERROR_NOT_SAME_DEVICE returned by MoveFileExW when flags == 0.
        assert!(is_cross_device(&io::Error::from_raw_os_error(17)));
        assert!(!is_cross_device(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }

    #[test]
    fn modified_source_during_copy_is_never_removed() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("source.bin");
        let dest = dir.path().join("destination");
        fs::create_dir(&dest).unwrap();
        fs::write(&from, vec![3u8; 1_000_000]).unwrap();
        let to = dest.join("source.bin");
        let changed = std::cell::Cell::new(false);
        let result = move_one_with_rename(
            &from,
            &to,
            &dest,
            &AtomicBool::new(false),
            &|progress| {
                if progress.done_bytes > 0 && !changed.replace(true) {
                    fs::write(&from, vec![9u8; 1_000_001]).unwrap();
                }
            },
            simulate_cross_volume,
        );
        assert!(result.is_err());
        assert_eq!(fs::metadata(&from).unwrap().len(), 1_000_001);
        assert!(
            !to.exists(),
            "A copy must not be published after source mutation"
        );
        assert_eq!(fs::read_dir(dest).unwrap().count(), 0);
    }

    fn simulate_cross_volume(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::CrossesDevices))
    }

    #[test]
    fn simulated_cross_volume_move_verifies_and_removes_only_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        let source = from.join("référence.txt");
        let bytes = vec![0xa5u8; 800_000];
        fs::write(&source, &bytes).unwrap();
        let target = to.join("référence.txt");
        let cancel = AtomicBool::new(false);
        move_one_with_rename(
            &source,
            &target,
            &to,
            &cancel,
            &|_| {},
            simulate_cross_volume,
        )
        .unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), bytes);
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
    }

    #[test]
    fn simulated_cross_volume_cancel_never_publishes_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("big.bin");
        let to = dir.path().join("to");
        fs::create_dir(&to).unwrap();
        fs::write(&source, vec![4u8; 2 * 1024 * 1024]).unwrap();
        let target = to.join("big.bin");
        let cancel = AtomicBool::new(false);
        let result = move_one_with_rename(
            &source,
            &target,
            &to,
            &cancel,
            &|p| {
                if p.done_bytes > 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            simulate_cross_volume,
        );
        assert!(result.is_err());
        assert_eq!(fs::metadata(source).unwrap().len(), 2 * 1024 * 1024);
        assert_eq!(fs::read_dir(to).unwrap().count(), 0);
    }

    #[test]
    fn directory_snapshot_detects_new_files_without_trashing_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("folder");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("empty")).unwrap();
        fs::write(source.join("important.txt"), b"KEEP").unwrap();
        let snapshot = snapshot_tree(&source).unwrap();
        assert!(snapshot.contains_key(Path::new("empty")));
        fs::write(source.join("new.txt"), b"NEW").unwrap();
        assert_ne!(snapshot_tree(&source).unwrap(), snapshot);
        assert_eq!(fs::read(source.join("important.txt")).unwrap(), b"KEEP");
    }

    #[test]
    fn non_cross_volume_error_never_triggers_fallback_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original.txt");
        let target_dir = dir.path().join("destination");
        fs::create_dir(&target_dir).unwrap();
        fs::write(&source, b"ORIGINAL").unwrap();
        let target = target_dir.join("original.txt");
        let error = move_one_with_rename(
            &source,
            &target,
            &target_dir,
            &AtomicBool::new(false),
            &|_| {},
            |_, _| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        )
        .unwrap_err();
        assert!(error.contains("Spostamento non riuscito"));
        assert_eq!(fs::read(source).unwrap(), b"ORIGINAL");
        assert_eq!(fs::read_dir(target_dir).unwrap().count(), 0);
    }

    #[test]
    fn verify_copy_detects_corruption_without_deleting_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        let target = dir.path().join("target.txt");
        fs::write(&source, b"ORIGINAL").unwrap();
        fs::write(&target, b"DIFFERNT").unwrap();
        let meta = fs::metadata(&source).unwrap();
        let result = verify_copy(&source, &target, &meta, &AtomicBool::new(false));
        assert!(result.unwrap_err().contains("Verifica byte fallita"));
        assert_eq!(fs::read(source).unwrap(), b"ORIGINAL");
    }

    #[test]
    fn source_changed_after_publication_is_kept_for_reconciliation() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        fs::write(&source, b"before").unwrap();
        let snapshot = fs::metadata(&source).unwrap();
        fs::write(&source, b"changed content").unwrap();
        assert!(!unchanged_source(&source, &snapshot));
    }

    #[test]
    fn cross_volume_folder_moves_nested_unicode_and_empty_directories_after_verification() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("área 漢字");
        let destination = dir.path().join("destination");
        let archived = dir.path().join("recycle-simulated");
        fs::create_dir_all(source.join("foto").join("empty")).unwrap();
        fs::create_dir(&destination).unwrap();
        let contents = vec![42u8; 600_000];
        fs::write(source.join("foto").join("file résumé.bin"), &contents).unwrap();
        fs::write(source.join("note.txt"), b"ok").unwrap();
        let target = destination.join("área 漢字");
        let called = std::cell::Cell::new(false);
        move_folder_across_volumes(
            &source,
            &target,
            &destination,
            &AtomicBool::new(false),
            &|_| {},
            |p| {
                assert!(target.join("foto").join("empty").is_dir());
                assert_eq!(
                    fs::read(target.join("foto").join("file résumé.bin")).unwrap(),
                    contents
                );
                called.set(true);
                fs::rename(p, &archived).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        assert!(called.get());
        assert!(!source.exists());
        assert_eq!(fs::read(archived.join("note.txt")).unwrap(), b"ok");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
    }

    #[test]
    fn cross_volume_folder_cancel_never_publishes_anything() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("folder");
        let destination = dir.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("data.bin"), vec![1u8; 2 * 1024 * 1024]).unwrap();
        let cancelled = AtomicBool::new(false);
        let recycled = std::cell::Cell::new(false);
        let result = move_folder_across_volumes(
            &source,
            &destination.join("folder"),
            &destination,
            &cancelled,
            &|p| {
                if p.done_bytes > 0 {
                    cancelled.store(true, Ordering::Relaxed);
                }
            },
            |_| {
                recycled.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!recycled.get());
        assert_eq!(
            fs::metadata(source.join("data.bin")).unwrap().len(),
            2 * 1024 * 1024
        );
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn cross_volume_folder_copy_corruption_detected_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("folder");
        let destination = dir.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("a.txt"), b"CORRECT").unwrap();
        fs::write(destination.join("unrelated.txt"), b"keep").unwrap();
        let expected = snapshot_tree(&source).unwrap();
        let bad_copy = dir.path().join("bad-copy");
        fs::create_dir(&bad_copy).unwrap();
        fs::write(bad_copy.join("a.txt"), b"WRONG!!").unwrap();
        let error =
            verify_folder_tree(&source, &bad_copy, &expected, &AtomicBool::new(false)).unwrap_err();
        assert!(error.contains("Verifica byte fallita"));
        assert_eq!(fs::read(source.join("a.txt")).unwrap(), b"CORRECT");
        assert_eq!(
            fs::read(destination.join("unrelated.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn cross_volume_folder_changed_mid_copy_preserves_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("folder");
        let destination = dir.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("data.bin"), vec![1u8; 1_000_000]).unwrap();
        let changed = std::cell::Cell::new(false);
        let recycle_called = std::cell::Cell::new(false);
        let result = move_folder_across_volumes(
            &source,
            &destination.join("folder"),
            &destination,
            &AtomicBool::new(false),
            &|p| {
                if p.done_bytes > 0 && !changed.replace(true) {
                    fs::write(source.join("new.txt"), b"New arrival").unwrap();
                }
            },
            |_| {
                recycle_called.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!recycle_called.get());
        assert_eq!(fs::read(source.join("new.txt")).unwrap(), b"New arrival");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn published_folder_failure_is_distinct_from_unpublished_failure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let dest = dir.path().join("dest");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(source.join("important.bin"), b"KEEP").unwrap();
        let target = dest.join("source");
        let error = move_folder_across_volumes(
            &source,
            &target,
            &dest,
            &AtomicBool::new(false),
            &|_| {},
            |_| Err("Cestino disabilitato".into()),
        )
        .unwrap_err();
        assert!(error.published);
        assert!(error.contains("Cestino disabilitato"));
        assert_eq!(fs::read(source.join("important.bin")).unwrap(), b"KEEP");
        assert_eq!(fs::read(target.join("important.bin")).unwrap(), b"KEEP");

        let before = MoveFailure::before("Annullato");
        assert!(!before.published);
        assert!(before.contains("Annullato"));
    }

    #[test]
    fn stop_preflight_detects_duplicate_names_before_any_copy() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("folder_a");
        let b = root.path().join("folder_b");
        let dest = root.path().join("destination");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::create_dir(&dest).unwrap();
        let first = a.join("identical.txt");
        let second = b.join("identical.txt");
        fs::write(&first, b"FIRST").unwrap();
        fs::write(&second, b"SECOND").unwrap();
        let result = execute(
            vec![first.clone(), second.clone()],
            dest.clone(),
            false,
            ConflictPolicy::Stop,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(result.contains("Conflitto nel batch"));
        assert!(!dest.join("identical.txt").exists());
        assert_eq!(fs::read(first).unwrap(), b"FIRST");
        assert_eq!(fs::read(second).unwrap(), b"SECOND");
    }

    #[test]
    fn mixed_batch_reports_skips_failures_and_unprocessed_items() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let kept = root.path().join("kept.txt");
        let missing = root.path().join("missing.txt");
        let skipped = root.path().join("skipped.txt");
        fs::write(&kept, b"copy").unwrap();
        fs::write(&skipped, b"skip").unwrap();
        fs::write(dest.join("skipped.txt"), b"existing").unwrap();
        let result = execute(
            vec![skipped, missing, kept.clone()],
            dest.clone(),
            false,
            ConflictPolicy::KeepBoth,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(result.contains("Completati 2/3"), "{result}");
        assert!(result.contains("falliti 1"), "{result}");
        assert!(result.contains("non elaborati 0"), "{result}");
        assert_eq!(fs::read(dest.join("kept.txt")).unwrap(), b"copy");
    }

    #[test]
    fn cross_volume_recycle_failure_reports_both_copies_without_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("file.txt"), b"SAFE").unwrap();
        let target = destination.join("source");
        let result = move_folder_across_volumes(
            &source,
            &target,
            &destination,
            &AtomicBool::new(false),
            &|_| {},
            |_| Err("Cestino indisponibile".into()),
        )
        .unwrap_err();
        assert!(result.contains("Cestino indisponibile"));
        assert!(result.contains("Verifica entrambe le posizioni"));
        assert_eq!(fs::read(source.join("file.txt")).unwrap(), b"SAFE");
        assert_eq!(fs::read(target.join("file.txt")).unwrap(), b"SAFE");
    }

    #[test]
    fn cross_volume_folder_rejects_link_without_publishing_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("folder");
        let destination = dir.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&source, source.join("cycle")).unwrap();
        }
        #[cfg(windows)]
        {
            // A file symlink may be unavailable on Windows without developer
            // mode. Missing symlink permissions leave this case for GUI QA.
            if std::os::windows::fs::symlink_file(dir.path(), source.join("cycle")).is_err() {
                return;
            }
        }
        let result = move_folder_across_volumes(
            &source,
            &destination.join("folder"),
            &destination,
            &AtomicBool::new(false),
            &|_| {},
            |_| Ok(()),
        );
        assert!(result.is_err());
        assert!(source.exists());
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn conflicts_skip_or_keep_both_without_overwrite() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("from");
        let dst = d.path().join("to");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        let f = src.join("a.txt");
        std::fs::write(&f, "NEW").unwrap();
        std::fs::write(dst.join("a.txt"), "OLD").unwrap();
        let cancel = AtomicBool::new(false);
        assert!(
            execute(
                vec![f.clone()],
                dst.clone(),
                false,
                ConflictPolicy::Stop,
                &cancel,
                |_| {}
            )
            .is_err()
        );
        execute(
            vec![f.clone()],
            dst.clone(),
            false,
            ConflictPolicy::Skip,
            &cancel,
            |_| {},
        )
        .unwrap();
        execute(
            vec![f.clone()],
            dst.clone(),
            false,
            ConflictPolicy::KeepBoth,
            &cancel,
            |_| {},
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "OLD");
        assert_eq!(
            std::fs::read_to_string(dst.join("a (1).txt")).unwrap(),
            "NEW"
        );
        assert!(f.exists());
    }
    #[test]
    fn cancellation_never_publishes_a_partial_copy() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("big.bin");
        let dst = d.path().join("dest");
        std::fs::create_dir(&dst).unwrap();
        std::fs::write(&src, vec![1u8; 1024 * 1024]).unwrap();
        let cancel = AtomicBool::new(false);
        let r = execute(
            vec![src.clone()],
            dst.clone(),
            false,
            ConflictPolicy::KeepBoth,
            &cancel,
            |p| {
                if p.done_bytes > 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(r.is_err());
        assert!(src.exists());
        assert_eq!(std::fs::read_dir(dst).unwrap().count(), 0);
    }
    #[test]
    fn invalid_destination_is_rejected_without_changes() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a");
        let dest = d.path().join("file");
        std::fs::write(&src, "A").unwrap();
        std::fs::write(&dest, "D").unwrap();
        assert!(
            execute(
                vec![src.clone()],
                dest.clone(),
                false,
                ConflictPolicy::Stop,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(src).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(dest).unwrap(), "D");
    }
    #[cfg(windows)]
    #[test]
    fn locked_source_is_preserved_and_staging_is_cleaned() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("locked.txt");
        let dest = d.path().join("dest");
        std::fs::write(&src, "ORIGINAL").unwrap();
        std::fs::create_dir(&dest).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&src)
            .unwrap();
        assert!(
            execute(
                vec![src.clone()],
                dest.clone(),
                false,
                ConflictPolicy::Stop,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_err()
        );
        drop(lock);
        assert_eq!(std::fs::read_to_string(src).unwrap(), "ORIGINAL");
        assert_eq!(std::fs::read_dir(dest).unwrap().count(), 0);
    }
    #[test]
    fn partial_move_report_identifies_completed_and_failed_items() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a");
        let dest = d.path().join("dest");
        std::fs::write(&src, "A").unwrap();
        std::fs::create_dir(&dest).unwrap();
        let error = execute(
            vec![src.clone(), d.path().join("missing")],
            dest.clone(),
            true,
            ConflictPolicy::Stop,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("Completati 1/2"));
        assert!(error.contains("missing"));
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(dest.join("a")).unwrap(), "A");
    }

    #[test]
    fn stop_checks_all_conflicts_before_any_publication() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("from");
        let dst = d.path().join("to");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        let clean = src.join("clean.txt");
        let conflicting = src.join("conflict.txt");
        std::fs::write(&clean, "CLEAN").unwrap();
        std::fs::write(&conflicting, "INCOMING").unwrap();
        std::fs::write(dst.join("conflict.txt"), "EXISTING").unwrap();

        let error = execute(
            vec![clean.clone(), conflicting.clone()],
            dst.clone(),
            false,
            ConflictPolicy::Stop,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("Nessun elemento trasferito"));
        assert!(!dst.join("clean.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dst.join("conflict.txt")).unwrap(),
            "EXISTING"
        );
        assert_eq!(std::fs::read_to_string(clean).unwrap(), "CLEAN");
        assert_eq!(std::fs::read_to_string(conflicting).unwrap(), "INCOMING");
    }

    #[test]
    fn keep_both_uses_next_free_suffix_for_repeated_names() {
        let d = tempfile::tempdir().unwrap();
        let src_a = d.path().join("from_a");
        let src_b = d.path().join("from_b");
        let dst = d.path().join("to");
        for dir in [&src_a, &src_b, &dst] {
            std::fs::create_dir(dir).unwrap();
        }
        let first = src_a.join("report.txt");
        let second = src_b.join("report.txt");
        std::fs::write(&first, "FIRST").unwrap();
        std::fs::write(&second, "SECOND").unwrap();
        std::fs::write(dst.join("report.txt"), "ORIGINAL").unwrap();

        execute(
            vec![first.clone(), second.clone()],
            dst.clone(),
            false,
            ConflictPolicy::KeepBoth,
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(dst.join("report.txt")).unwrap(),
            "ORIGINAL"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("report (1).txt")).unwrap(),
            "FIRST"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("report (2).txt")).unwrap(),
            "SECOND"
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "FIRST");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "SECOND");
    }

    #[test]
    fn copying_directory_into_descendant_leaves_no_staging() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("parent");
        let dst = src.join("child");
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(src.join("keep.txt"), "PRESERVE").unwrap();

        assert!(
            execute(
                vec![src.clone()],
                dst.clone(),
                false,
                ConflictPolicy::KeepBoth,
                &AtomicBool::new(false),
                |_| {},
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(src.join("keep.txt")).unwrap(),
            "PRESERVE"
        );
        assert_eq!(std::fs::read_dir(dst).unwrap().count(), 0);
    }
}
