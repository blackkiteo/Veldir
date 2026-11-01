//! Native filesystem event watcher. Never enumerates the tree or polls contents.
//! Watches only the immediate children of each open folder. UI debounces bursts.
use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

pub struct FolderWatcher {
    commands: Sender<Vec<PathBuf>>,
}

impl FolderWatcher {
    /// Callback is invoked from notify's background event thread.
    pub fn start(on_change: impl Fn(PathBuf) + Send + 'static) -> Self {
        let (commands, changes) = mpsc::channel::<Vec<PathBuf>>();
        std::thread::spawn(move || {
            let mut watcher =
                match notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    if let Ok(event) = result {
                        if matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Modify(_)
                                | EventKind::Remove(_)
                                | EventKind::Any
                        ) {
                            for path in event.paths {
                                on_change(path);
                            }
                        }
                    }
                }) {
                    Ok(w) => w,
                    Err(_) => return,
                };
            let mut active = BTreeSet::<PathBuf>::new();
            while let Ok(next) = changes.recv() {
                let requested: BTreeSet<_> = next.into_iter().collect();
                for path in active.difference(&requested) {
                    let _ = watcher.unwatch(path);
                }
                let mut installed = BTreeSet::new();
                for path in &requested {
                    if active.contains(path)
                        || watcher.watch(path, RecursiveMode::NonRecursive).is_ok()
                    {
                        installed.insert(path.clone());
                    }
                }
                active = installed;
            }
        });
        Self { commands }
    }

    pub fn set_paths(&self, paths: Vec<PathBuf>) {
        let _ = self.commands.send(paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observer_accepts_empty_watch_set() {
        let watcher = FolderWatcher::start(|_| {});
        watcher.set_paths(Vec::new());
    }
}
