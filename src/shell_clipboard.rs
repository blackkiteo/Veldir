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


/// Export normal existing paths to the system clipboard as CF_HDROP, so Explorer
/// can paste them. This only advertises COPIES; no "preferred drop effect" of MOVE.
#[cfg(windows)]
pub fn copy_files_to_explorer(paths: &[PathBuf]) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{copy_nonoverlapping, null_mut};

    #[link(name="user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, handle: *mut c_void) -> *mut c_void;
    }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
        fn GlobalFree(handle: *mut c_void) -> *mut c_void;
    }

    if paths.is_empty() { return Err("Nessun file selezionato".into()); }
    if paths.len() > 500 { return Err("Esporta al massimo 500 percorsi".into()); }
    let mut names: Vec<Vec<u16>> = Vec::new();
    let mut units: usize = 1; // final extra NUL for MULTI_SZ
    for path in paths {
        if !path.exists() { return Err(format!("Sorgente non disponibile: {}",path.display())); }
        // Use an absolute path: Explorer may be running with a different working directory.
        let absolute = path.canonicalize().map_err(|e| e.to_string())?;
        let name: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if name.is_empty() || name.contains(&0) { return Err("Percorso Windows non valido".into()); }
        units=units.checked_add(name.len()+1).ok_or("Troppe informazioni nella clipboard")?;
        names.push(name);
    }
    // DROPFILES: 20-byte struct, with DWORD pFiles=20 and BOOL fWide=1.
    let bytes=20usize.checked_add(units.checked_mul(2).ok_or("Percorsi troppo lunghi")?)
        .ok_or("Percorsi troppo lunghi")?;
    if bytes > 4*1024*1024 { return Err("Percorsi selezionati troppo lunghi".into()); }

    struct Allocation(*mut c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if !self.0.is_null() { unsafe { GlobalFree(self.0); } }
        }
    }
    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) { unsafe { CloseClipboard(); } }
    }
    // GMEM_MOVEABLE|GMEM_ZEROINIT is required by SetClipboardData.
    let hglobal=unsafe { GlobalAlloc(0x0002 | 0x0040,bytes) };
    if hglobal.is_null() { return Err("Memoria clipboard insufficiente".into()); }
    let mut allocation=Allocation(hglobal);
    let ptr=unsafe {GlobalLock(hglobal)} as *mut u8;
    if ptr.is_null() { return Err("Impossibile preparare la clipboard".into()); }
    unsafe {
        // Zeroed memory already contains POINT(0,0), fNC=false and terminator.
        (ptr as *mut u32).write_unaligned(20);
        (ptr.add(16) as *mut i32).write_unaligned(1); // wide UTF-16
        let mut offset=20usize;
        for name in names {
            copy_nonoverlapping(name.as_ptr() as *const u8, ptr.add(offset), name.len()*2);
            offset+= (name.len()+1)*2; // zero terminator from GMEM_ZEROINIT
        }
        GlobalUnlock(hglobal);
    }
    // Clipboard's new owner must be a real window, not NULL.
    let owner=unsafe { GetForegroundWindow() };
    if owner.is_null() {return Err("Apri Veldir in primo piano per esportare i file".into());}
    if unsafe { OpenClipboard(owner) } == 0 { return Err("Clipboard occupata: riprova".into()); }
    let _clipboard=ClipboardGuard;
    if unsafe { EmptyClipboard() } == 0 { return Err("Impossibile svuotare la clipboard".into()); }
    if unsafe { SetClipboardData(15,hglobal) }.is_null() {
        return Err("Impossibile pubblicare i percorsi nella clipboard".into());
    }
    // From this point on, Windows owns and frees the allocation.
    allocation.0=null_mut();
    Ok(())
}

#[cfg(not(windows))]
pub fn copy_files_to_explorer(_paths: &[PathBuf]) -> Result<(), String> {
    Err("L'esportazione file nella clipboard è disponibile solo su Windows".into())
}

#[cfg(all(windows,test))]
mod tests {
    use super::*;
    #[test]
    fn export_rejects_empty_paths_without_modifying_clipboard() {
        assert!(copy_files_to_explorer(&[]).is_err());
    }
    #[test]
    fn export_rejects_missing_files_without_modifying_clipboard() {
        let path=std::env::temp_dir().join("veldir-missing-file-for-test-94f3d6a");
        assert!(copy_files_to_explorer(&[path]).is_err());
    }
}
