//! Windows Shell icons, rendered into an RGBA bitmap.
//! SHGFI_USEFILEATTRIBUTES avoids reading file contents or traversing networks.
//! Each file extension is extracted once and the UI caches its texture.
use crate::image_view::Bitmap;

pub fn key(name: &str, is_dir: bool) -> String {
    if is_dir { return "folder".into(); }
    let extension=std::path::Path::new(name).extension()
        .and_then(|e| e.to_str()).unwrap_or("");
    format!("file:{}",extension.to_ascii_lowercase())
}

#[cfg(windows)]
pub fn load(key: &str) -> Result<Bitmap,String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShellInfo {
        icon: *mut c_void,
        icon_index: i32,
        attributes: u32,
        display: [u16;260],
        kind: [u16;80],
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32, width: i32, height: i32,
        planes: u16, bits_per_pixel: u16,
        compression: u32, image_size: u32,
        x_pixels_per_meter: i32, y_pixels_per_meter: i32,
        colors_used: u32, colors_important: u32,
    }
    #[repr(C)]
    struct BitmapInfo { header: BitmapInfoHeader, color: [u32;1] }

    #[link(name="shell32")]
    unsafe extern "system" {
        fn SHGetFileInfoW(path: *const u16, attributes: u32, info: *mut ShellInfo,
                          cb_file_info: u32, flags: u32) -> usize;
    }
    #[link(name="user32")]
    unsafe extern "system" {
        fn DrawIconEx(dc: *mut c_void, x: i32, y: i32, icon: *mut c_void,
                      width: i32, height: i32, step: u32,
                      brush: *mut c_void, flags: u32) -> i32;
        fn DestroyIcon(icon: *mut c_void) -> i32;
    }
    #[link(name="gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
        fn DeleteDC(dc: *mut c_void) -> i32;
        fn CreateDIBSection(dc: *mut c_void, bmi: *const BitmapInfo, usage: u32,
                            bits: *mut *mut c_void, section: *mut c_void,
                            offset: u32) -> *mut c_void;
        fn SelectObject(dc: *mut c_void, object: *mut c_void) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
    }
    let folder=key == "folder";
    let suffix=key.strip_prefix("file:").unwrap_or("");
    let dummy=if folder { "veldir-folder".to_owned() }
        else if suffix.is_empty() { "veldir-unknown".to_owned() }
        else { format!("veldir-file.{suffix}") };
    let wide: Vec<u16>=std::ffi::OsStr::new(&dummy).encode_wide().chain([0]).collect();
    let mut info: ShellInfo=unsafe { std::mem::zeroed() };
    const SHGFI_ICON: u32=0x00000100;
    const SHGFI_SMALLICON: u32=0x00000001;
    const SHGFI_USEFILEATTRIBUTES: u32=0x00000010;
    let attrs=if folder {0x10u32} else {0x80u32}; // DIRECTORY or NORMAL
    let ok=unsafe {SHGetFileInfoW(wide.as_ptr(),attrs,&mut info,
        std::mem::size_of::<ShellInfo>() as u32,
        SHGFI_ICON|SHGFI_SMALLICON|SHGFI_USEFILEATTRIBUTES)};
    if ok == 0 || info.icon.is_null() {return Err("Icona Shell non disponibile".into());}
    const SIZE: usize=32;
    let dc=unsafe {CreateCompatibleDC(std::ptr::null_mut())};
    if dc.is_null() {
        unsafe {DestroyIcon(info.icon); }
        return Err("Device context per icona non disponibile".into());
    }
    let bmi=BitmapInfo {
        header:BitmapInfoHeader {
            size:40,width:SIZE as i32,height:-(SIZE as i32),
            planes:1,bits_per_pixel:32,compression:0,image_size:(SIZE*SIZE*4) as u32,
            x_pixels_per_meter:0,y_pixels_per_meter:0,colors_used:0,colors_important:0,
        },color:[0]
    };
    let mut ptr: *mut c_void=std::ptr::null_mut();
    let bitmap=unsafe {CreateDIBSection(dc,&bmi,0,&mut ptr,std::ptr::null_mut(),0)};
    if bitmap.is_null() || ptr.is_null() {
        unsafe {DeleteDC(dc);DestroyIcon(info.icon);}
        return Err("Impossibile preparare l'immagine dell'icona".into());
    }
    let previous=unsafe {SelectObject(dc,bitmap)};
    let success=unsafe {DrawIconEx(dc,0,0,info.icon,SIZE as i32,SIZE as i32,
        0,std::ptr::null_mut(),3)};
    let raw=if success != 0 {
        unsafe {std::slice::from_raw_parts(ptr as *const u8,SIZE*SIZE*4).to_vec()}
    } else {Vec::new()};
    unsafe {
        SelectObject(dc,previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(info.icon);
    }
    if raw.is_empty() {return Err("Rendering icona Shell fallito".into());}
    // DIB BGRA -> egui RGBA; the transparency from the Shell is preserved.
    let mut rgba=raw;
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0,2);
    }
    Ok(Bitmap{width:SIZE,height:SIZE,rgba})
}

#[cfg(not(windows))]
pub fn load(_key: &str) -> Result<Bitmap,String> {
    Err("Le icone di sistema sono disponibili solo su Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_cache_reuses_the_same_extension() {
        assert_eq!(key("a.PDF",false),key("report.pdf",false));
        assert_eq!(key("Pictures",true),"folder");
        assert_ne!(key("Photo.png",false),key("doc.txt",false));
    }
}
