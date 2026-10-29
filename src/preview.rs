//! Read-only capped text preview; never loads a full large file into memory.
use std::fs;
use std::io::{Read};
use std::path::{Path,PathBuf};

#[derive(Clone, Debug)]
pub struct Details {
    pub path: PathBuf,
    pub kind: String,
    pub bytes: u64,
    pub text: Option<String>,
    pub note: Option<String>,
}

const MAX_BYTES: usize = 32 * 1024;
pub fn details(path: &Path) -> Result<Details, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let kind = if meta.is_dir() { "Cartella".to_owned() }
        else if meta.file_type().is_symlink() { "Collegamento".to_owned() }
        else { path.extension().and_then(|s| s.to_str()).unwrap_or("File").to_ascii_uppercase() };
    let mut info = Details { path: path.to_owned(), kind, bytes: meta.len(), text: None, note: None };
    if !meta.is_file() { return Ok(info); }
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "pdf" {
        match crate::pdf_view::inspect(path) {
            Ok(result) => {
                info.kind = format!("PDF · {} pagine", result.pages);
                info.text = result.excerpt;
                info.note = Some(result.note);
            }
            Err(error) => info.note = Some(error),
        }
        return Ok(info);
    }
    if !["txt","md","rs","json","csv","xml","toml","log","ini","yaml","yml","html","css","js","ts","py","ps1","bat"].contains(&ext.as_str()) {
        info.note = Some("Anteprima testuale non disponibile per questo formato".into());
        return Ok(info);
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.take(MAX_BYTES as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.iter().any(|b| *b == 0) { info.note = Some("File binario: anteprima non disponibile".into()); return Ok(info); }
    info.text = Some(String::from_utf8_lossy(&buf).to_string());
    if meta.len() > MAX_BYTES as u64 { info.note = Some("Anteprima limitata ai primi 32 KiB".into()); }
    Ok(info)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_bounded() {
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("notes.txt");
        fs::write(&path, vec![b'x'; 80_000]).unwrap();
        let d=details(&path).unwrap();
        assert_eq!(d.text.unwrap().len(), 32 * 1024);
        assert!(d.note.is_some());
    }
}