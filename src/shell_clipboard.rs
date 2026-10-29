//! Explicit, read-only import of file paths from Explorer's CF_HDROP clipboard.
//! No moving, no clipboard mutations, and no assumption that Explorer's Cut means Move.
use std::path::PathBuf;

#[cfg(windows)]
pub fn files_from_explorer() -> Result<Vec<PathBuf>, String> {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::null_mut;

    #[link(name="user32")]
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
    }
    #[link(name="shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(hdrop: *mut c_void, index: u32, buf: *mut u16, chars: u32) -> u32;
    }

    struct ClipboardOpen;
    impl Drop for ClipboardOpen {
        fn drop(&mut self) { unsafe { CloseClipboard(); } }
    }
    struct MemoryLock(*mut c_void);
    impl Drop for MemoryLock {
        fn drop(&mut self) { unsafe { GlobalUnlock(self.0); } }
    }
    // OpenClipboard is deliberately short-lived and performed only on explicit user action.
    if unsafe { OpenClipboard(null_mut()) } == 0 {
        return Err("Clipboard Windows occupata: riprova".into());
    }
    let _clipboard = ClipboardOpen;
    // Standard Win32 clipboard format: CF_HDROP = 15.
    let handle = unsafe { GetClipboardData(15) };
    if handle.is_null() {
        return Err("La clipboard non contiene file copiati da Esplora file".into());
    }
    let locked = unsafe { GlobalLock(handle) };
    if locked.is_null() { return Err("Impossibile leggere i percorsi nella clipboard".into()); }
    let _lock = MemoryLock(handle);
    let count = unsafe { DragQueryFileW(locked, u32::MAX, null_mut(), 0) };
    if count == 0 { return Err("Nessun file da importare".into()); }
    if count > 500 { return Err("Seleziona al massimo 500 file per volta".into()); }
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let len = unsafe { DragQueryFileW(locked, index, null_mut(), 0) };
        if len == 0 || len > 32767 { continue; }
        let mut buffer=vec![0u16;(len+1) as usize];
        let written=unsafe { DragQueryFileW(locked, index, buffer.as_mut_ptr(), len+1) };
        if written == 0 || written > len { continue; }
        let path=PathBuf::from(OsString::from_wide(&buffer[..written as usize]));
        if path.exists() { paths.push(path); }
    }
    if paths.is_empty() { return Err("I file della clipboard non sono più accessibili".into()); }
    Ok(paths)
}

#[cfg(not(windows))]
pub fn files_from_explorer() -> Result<Vec<PathBuf>, String> {
    Err("La clipboard dei file di Esplora è disponibile solo su Windows".into())
}
