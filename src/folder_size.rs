//! Non-blocking folder size worker: logical sum of regular-file lengths.
//! Never follows directory symlinks, Windows junctions or other reparse points.
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const MAX_ENTRIES: usize = 75_000;
const MAX_DURATION: Duration = Duration::from_secs(3);

#[cfg(windows)]
fn is_reparse_point(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_meta: &fs::Metadata) -> bool {
    false
}

/// Only call on a background worker, never on the UI thread.
/// Limits stop a large/remote folder from consuming unlimited disk time.
/// If a folder cannot be fully counted, return an error rather than a
/// misleading partial size. A single slow filesystem call cannot be interrupted.
pub fn calculate(folder: &Path) -> Result<u64, String> {
    let started = Instant::now();
    let mut stack = vec![folder.to_path_buf()];
    let mut visited = 0usize;
    let mut total = 0u64;
    while let Some(path) = stack.pop() {
        if started.elapsed() >= MAX_DURATION {
            return Err("Calcolo interrotto: cartella troppo lenta o grande".into());
        }
        visited += 1;
        if visited > MAX_ENTRIES {
            return Err("Troppi elementi: dimensione non calcolata".into());
        }
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Impossibile leggere {}: {e}", path.display()))?;
        if meta.file_type().is_symlink() || is_reparse_point(&meta) {
            return Err(format!(
                "Collegamento o junction non conteggiato: {}",
                path.display()
            ));
        }
        if meta.is_dir() {
            let children = fs::read_dir(&path)
                .map_err(|e| format!("Impossibile aprire {}: {e}", path.display()))?;
            for child in children {
                if started.elapsed() >= MAX_DURATION {
                    return Err("Calcolo interrotto: cartella troppo lenta o grande".into());
                }
                stack.push(
                    child
                        .map_err(|e| format!("Errore lettura cartella: {e}"))?
                        .path(),
                );
                if visited + stack.len() > MAX_ENTRIES {
                    return Err("Troppi elementi: dimensione non calcolata".into());
                }
            }
        } else if meta.is_file() {
            total = total
                .checked_add(meta.len())
                .ok_or("Dimensione troppo grande per essere rappresentata")?;
        } else {
            return Err(format!("Elemento non supportato: {}", path.display()));
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn nested_files_are_summed_without_counting_directory_metadata() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("A")).unwrap();
        fs::create_dir(root.path().join("A").join("B")).unwrap();
        fs::write(root.path().join("file.txt"), b"12345").unwrap();
        fs::write(root.path().join("A").join("more.bin"), [7u8; 11]).unwrap();
        fs::write(root.path().join("A").join("B").join("zero"), b"").unwrap();
        assert_eq!(calculate(root.path()).unwrap(), 16);
        assert_eq!(calculate(&root.path().join("A")).unwrap(), 11);
        assert_eq!(calculate(&root.path().join("A").join("B")).unwrap(), 0);
    }

    #[test]
    fn missing_and_deleted_folders_fail_instead_of_reporting_zero() {
        let root = tempfile::tempdir().unwrap();
        assert!(calculate(&root.path().join("missing")).is_err());
        let path = root.path().to_path_buf();
        drop(root);
        assert!(calculate(&path).is_err());
    }

    #[test]
    fn counts_file_bytes_not_allocated_disk_blocks() {
        let root = tempfile::tempdir().unwrap();
        let mut file = fs::File::create(root.path().join("file")).unwrap();
        file.write_all(&[0u8; 257]).unwrap();
        file.flush().unwrap();
        assert_eq!(calculate(root.path()).unwrap(), 257);
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlinks_are_rejected_without_traversal() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        symlink(root.path(), root.path().join("cycle")).unwrap();
        assert!(calculate(root.path()).unwrap_err().contains("Collegamento"));
    }
}
