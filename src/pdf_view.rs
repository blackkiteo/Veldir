//! Read-only bounded PDF text and page-count inspection.
//! This is not a visual renderer; scanned/image-only PDFs need a separate renderer.
use std::fs;
use std::path::Path;

const MAX_PDF_BYTES: u64 = 12 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 2000;
const MAX_VISIBLE_CHARS: usize = 8_000;
const MAX_DECOMPRESSED_PAGE: usize = 512 * 1024;

#[derive(Debug)]
pub struct PdfText {
    pub pages: usize,
    pub excerpt: Option<String>,
    pub note: String,
}

pub fn inspect(path: &Path) -> Result<PdfText, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Anteprima PDF disponibile solo per file regolari".into());
    }
    if meta.len() > MAX_PDF_BYTES {
        return Err("PDF oltre 12 MiB: aprilo con il lettore predefinito".into());
    }
    let doc =
        lopdf::Document::load(path).map_err(|_| "PDF non leggibile o danneggiato".to_owned())?;
    if doc.is_encrypted() {
        return Err("PDF protetto: aprilo con il lettore predefinito".into());
    }
    let page_map = doc.get_pages();
    let pages = page_map.len();
    if pages > MAX_PDF_PAGES {
        return Err("PDF con troppe pagine per l'anteprima".into());
    }
    if pages == 0 {
        return Ok(PdfText {
            pages,
            excerpt: None,
            note: "PDF senza pagine".into(),
        });
    }
    let first_pages: Vec<u32> = page_map.keys().copied().take(2).collect();
    let text = doc
        .extract_text_with_limit(&first_pages, MAX_DECOMPRESSED_PAGE)
        .map_err(|_| {
            "Impossibile estrarre il testo; apri il PDF con il lettore predefinito".to_owned()
        })?;
    let text = text.trim();
    let excerpt: String = text.chars().take(MAX_VISIBLE_CHARS).collect();
    let note = if excerpt.is_empty() {
        "PDF senza testo estraibile nelle prime due pagine (potrebbe essere una scansione)".into()
    } else if pages > 2 {
        format!("Estratto delle prime 2 pagine su {pages}; testo limitato a 8.000 caratteri")
    } else {
        "Testo PDF estratto: non è un rendering grafico della pagina".into()
    };
    Ok(PdfText {
        pages,
        excerpt: if excerpt.is_empty() {
            None
        } else {
            Some(excerpt)
        },
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_oversized_pdf_before_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.pdf");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_PDF_BYTES + 1).unwrap();
        assert!(inspect(&path).unwrap_err().contains("12 MiB"));
    }
    #[test]
    fn rejects_unreadable_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.pdf");
        fs::write(&path, b"This is not a PDF").unwrap();
        assert!(inspect(&path).is_err());
    }
}
