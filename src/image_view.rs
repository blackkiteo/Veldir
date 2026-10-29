//! Bounded image loading for previews and lazy thumbnails.
//! Decoding is performed in a worker thread, never on the UI thread.
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PIXELS: u64 = 30_000_000;

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp"))
        .unwrap_or(false)
}

/// max_side=40 for a file-list thumbnail, max_side=1200 for a preview.
pub fn load(path: &Path, max_side: u32) -> Result<Bitmap, String> {
    if !supported(path) { return Err("Formato immagine non supportato".into()); }
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Non è un file immagine regolare".into());
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err("Immagine troppo grande per l'anteprima (max 32 MiB)".into());
    }
    let reader = image::ImageReader::open(path).map_err(|e| e.to_string())?
        .with_guessed_format().map_err(|e| e.to_string())?;
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Dimensioni immagine non supportate (max 30 megapixel)".into());
    }
    let decoded = image::ImageReader::open(path).map_err(|e| e.to_string())?
        .with_guessed_format().map_err(|e| e.to_string())?
        .decode().map_err(|e| e.to_string())?;
    // image::thumbnail may upscale small sources: keep original size as upper bound.
    let bounded_side = max_side.clamp(24, 1200).min(width.max(height));
    let scaled = decoded.thumbnail(bounded_side, bounded_side).to_rgba8();
    let (width, height) = scaled.dimensions();
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        rgba: scaled.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downscaling_is_bounded_and_preserves_aspect_ratio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("photo.png");
        image::RgbImage::new(320, 160).save(&path).unwrap();
        let small = load(&path, 40).unwrap();
        assert_eq!((small.width, small.height), (40, 20));
        assert_eq!(small.rgba.len(), 40 * 20 * 4);
        let large = load(&path, 1200).unwrap();
        assert_eq!((large.width, large.height), (320,160));
    }
    #[test]
    fn skips_large_images_before_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.png");
        std::fs::write(&path, vec![0; 32 * 1024 * 1024 + 1]).unwrap();
        assert!(load(&path, 40).unwrap_err().contains("troppo grande"));
    }
}
