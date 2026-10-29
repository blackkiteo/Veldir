use crate::fs_ops::{self, Job, Progress};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Stop,
    Skip,
    KeepBoth,
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
    let total_bytes = if cut {
        0
    } else {
        sources
            .iter()
            .filter(|s| {
                policy != ConflictPolicy::Skip
                    || s.file_name()
                        .is_some_and(|n| destination.join(n).symlink_metadata().is_err())
            })
            .filter_map(|s| fs_ops::tree_size(s).ok())
            .fold(0u64, |sum, n| sum.saturating_add(n))
    };
    let mut completed_bytes = 0u64;
    let mut done = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();
    let mut details = Vec::new();
    // Stop policy validates all conflicts before publishing the first file.
    if policy == ConflictPolicy::Stop {
        for source in &sources {
            let name = source.file_name().ok_or("Nome sorgente non valido")?;
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
            unique_target(&destination, name.to_string_lossy().as_ref())?
        } else {
            original
        };
        let result = if cut {
            fs_ops::rename_no_replace(&source, &target).map_err(|e| {
                format!("Spostamento non riuscito: {e}. Per dischi diversi usare Copia.")
            })
        } else {
            // Copy into a private sibling, then atomically publish without replacement.
            let staging = tempfile::Builder::new()
                .prefix(".veldir-transfer-")
                .tempdir_in(&destination)
                .map_err(|e| e.to_string())?;
            let progress = |p: Progress| {
                notify(Progress {
                    done_items: done + skipped + errors.len(),
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
        };
        match result {
            Ok(_) => {
                completed_bytes = completed_bytes.saturating_add(if cut {
                    0
                } else {
                    fs_ops::tree_size(&source).unwrap_or(0)
                });
                done += 1;
                details.push(format!(
                    "Completato: {} → {}",
                    source.display(),
                    target.display()
                ));
            }
            Err(error) => {
                errors.push(format!("{}: {error}", source.display()));
                if policy == ConflictPolicy::Stop {
                    break;
                }
            }
        }
        notify(Progress {
            done_items: done + skipped + errors.len(),
            total_items: total,
            done_bytes: completed_bytes,
            total_bytes,
        });
    }
    let summary = format!(
        "Completati {done}/{total} · saltati {skipped} · errori {}
{}
{}",
        errors.len(),
        details.join(
            "
"
        ),
        errors.join(
            "
"
        )
    );
    if errors.is_empty() && !cancelled.load(Ordering::Relaxed) {
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
