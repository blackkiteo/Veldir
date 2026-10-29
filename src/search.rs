//! Bounded, read-only recursive search. No symlink/junction traversal.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct Hit {
    pub path: PathBuf,
    pub is_dir: bool,
}

fn fuzzy(text: &str, query: &str) -> bool {
    let mut chars = text.chars().flat_map(char::to_lowercase);
    query.chars().flat_map(char::to_lowercase).all(|needle| chars.by_ref().any(|c| c == needle))
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x0000_0400 != 0
    }
    #[cfg(not(windows))]
    { let _ = meta; false }
}

/// Returns up to limit matches. The search can be cancelled between directory entries.
/// Inaccessible directories are skipped; no changes are made to the filesystem.
pub fn find(root: &Path, query: &str, limit: usize, cancelled: &AtomicBool) -> Result<Vec<Hit>, String> {
    let query = query.trim();
    if query.is_empty() { return Ok(Vec::new()); }
    if !root.is_dir() { return Err("Cartella di ricerca non accessibile".into()); }
    let mut stack = vec![root.to_path_buf()];
    let mut hits = Vec::new();
    let mut visited = 0usize;
    const MAX_VISITED: usize = 250_000;
    while let Some(dir) = stack.pop() {
        if cancelled.load(Ordering::Relaxed) { return Err("Ricerca annullata".into()); }
        let iter = match fs::read_dir(dir) { Ok(v) => v, Err(_) => continue };
        for item in iter.flatten() {
            if cancelled.load(Ordering::Relaxed) { return Err("Ricerca annullata".into()); }
            visited += 1;
            if visited > MAX_VISITED {
                return Err("Limite di 250.000 elementi raggiunto: affina la ricerca".into());
            }
            let meta = match fs::symlink_metadata(item.path()) { Ok(v) => v, Err(_) => continue };
            if meta.file_type().is_symlink() || is_reparse(&meta) { continue; }
            let path = item.path();
            let is_dir = meta.is_dir();
            let name = item.file_name().to_string_lossy().into_owned();
            if fuzzy(&name, query) {
                hits.push(Hit { path: path.clone(), is_dir });
                if hits.len() >= limit.max(1) { return Ok(hits); }
            }
            if is_dir { stack.push(path); }
        }
    }
    hits.sort_unstable_by(|a,b| a.path.cmp(&b.path));
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_is_case_insensitive_and_ordered() {
        assert!(fuzzy("Annual_Report_2026.pdf", "arp26"));
        assert!(fuzzy("Caffè", "caffè"));
        assert!(!fuzzy("report", "trop"));
    }
    #[test]
    fn bounded_search_and_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("invoice-001.txt"), "a").unwrap();
        fs::write(dir.join("invoice-002.txt"), "b").unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(find(tmp.path(), "invoice", 1, &cancel).unwrap().len(), 1);
        cancel.store(true, Ordering::Relaxed);
        assert!(find(tmp.path(), "invoice", 10, &cancel).is_err());
    }
}