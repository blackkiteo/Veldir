//! Explicit, read-only import of file paths from Explorer's CF_HDROP clipboard.
//! No moving, no clipboard mutations, and no assumption that Explorer's Cut means Move.
use std::path::PathBuf;

/**
 * Windows increments this when another app replaces the clipboard. A local
 * Ctrl+X must not silently win over a newer Ctrl+C performed in Explorer.
 * None means the API cannot identify the clipboard's generation.
 */
#[cfg(windows)]
pub fn sequence_number() -> Option<u32> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }
    match unsafe { GetClipboardSequenceNumber() } {
        0 => None,
        n => Some(n),
    }
}

#[cfg(not(windows))]
pub fn sequence_number() -> Option<u32> {
    None
}

/// Whether CF_HDROP file paths are currently advertised by Windows.
#[cfg(windows)]
pub fn contains_files() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }
    unsafe { IsClipboardFormatAvailable(15) != 0 }
}

#[cfg(not(windows))]
pub fn contains_files() -> bool {
    false
}

#[cfg(windows)]
pub fn files_from_explorer() -> Result<Vec<PathBuf>, String> {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::null_mut;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(hdrop: *mut c_void, index: u32, buf: *mut u16, chars: u32) -> u32;
    }

    struct ClipboardOpen;
    impl Drop for ClipboardOpen {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    struct MemoryLock(*mut c_void);
    impl Drop for MemoryLock {
        fn drop(&mut self) {
            unsafe {
                GlobalUnlock(self.0);
            }
        }
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
    if locked.is_null() {
        return Err("Impossibile leggere i percorsi nella clipboard".into());
    }
    let _lock = MemoryLock(handle);
    let count = unsafe { DragQueryFileW(locked, u32::MAX, null_mut(), 0) };
    if count == 0 {
        return Err("Nessun file da importare".into());
    }
    if count > 500 {
        return Err("Seleziona al massimo 500 file per volta".into());
    }
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let len = unsafe { DragQueryFileW(locked, index, null_mut(), 0) };
        if len == 0 || len > 32767 {
            continue;
        }
        let mut buffer = vec![0u16; (len + 1) as usize];
        let written = unsafe { DragQueryFileW(locked, index, buffer.as_mut_ptr(), len + 1) };
        if written == 0 || written > len {
            continue;
        }
        let path = PathBuf::from(OsString::from_wide(&buffer[..written as usize]));
        if path.exists() {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err("I file della clipboard non sono più accessibili".into());
    }
    Ok(paths)
}

#[cfg(not(windows))]
pub fn files_from_explorer() -> Result<Vec<PathBuf>, String> {
    Err("La clipboard dei file di Esplora è disponibile solo su Windows".into())
}

/// Build the exact DROPFILES payload used for CF_HDROP. This is kept separate
/// from Win32 clipboard ownership, so layout and Unicode can be tested without
/// changing the clipboard of the build runner or the user's desktop.
#[cfg(windows)]
fn encode_dropfiles(paths: &[PathBuf]) -> Result<Vec<u8>, String> {
    use std::os::windows::ffi::OsStrExt;

    if paths.is_empty() {
        return Err("Nessun file selezionato".into());
    }
    if paths.len() > 500 {
        return Err("Esporta al massimo 500 percorsi".into());
    }
    let mut wide_paths: Vec<Vec<u16>> = Vec::with_capacity(paths.len());
    let mut units: usize = 1; // final extra NUL for MULTI_SZ
    for path in paths {
        if !path.exists() {
            return Err(format!("Sorgente non disponibile: {}", path.display()));
        }
        // Explorer may run with a different current directory.
        let absolute = path.canonicalize().map_err(|e| e.to_string())?;
        let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        if wide.is_empty() || wide.contains(&0) {
            return Err("Percorso Windows non valido".into());
        }
        units = units
            .checked_add(wide.len() + 1)
            .ok_or("Troppe informazioni nella clipboard")?;
        wide_paths.push(wide);
    }
    let bytes = 20usize
        .checked_add(units.checked_mul(2).ok_or("Percorsi troppo lunghi")?)
        .ok_or("Percorsi troppo lunghi")?;
    if bytes > 4 * 1024 * 1024 {
        return Err("Percorsi selezionati troppo lunghi".into());
    }
    // DROPFILES = POINT(8), pFiles(4), fNC(4), fWide(4).
    // Use little-endian bytes and NUL-initialization for the UTF-16 MULTI_SZ.
    let mut data = vec![0u8; bytes];
    data[..4].copy_from_slice(&20u32.to_le_bytes());
    data[16..20].copy_from_slice(&1u32.to_le_bytes());
    let mut cursor = 20;
    for wide in wide_paths {
        for unit in wide {
            data[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
            cursor += 2;
        }
        cursor += 2; // terminating NUL after each path
    }
    Ok(data)
}

/// Export normal existing paths to the system clipboard as CF_HDROP, so Explorer
/// can paste them. This only advertises COPIES; no "preferred drop effect" of MOVE.
#[cfg(windows)]
pub fn copy_files_to_explorer(paths: &[PathBuf]) -> Result<(), String> {
    use std::ffi::c_void;
    use std::ptr::{copy_nonoverlapping, null_mut};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn OpenClipboard(hwnd: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, handle: *mut c_void) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
        fn GlobalFree(handle: *mut c_void) -> *mut c_void;
    }

    let payload = encode_dropfiles(paths)?;

    struct Allocation(*mut c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    GlobalFree(self.0);
                }
            }
        }
    }
    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    // GMEM_MOVEABLE|GMEM_ZEROINIT is required by SetClipboardData.
    let hglobal = unsafe { GlobalAlloc(0x0002 | 0x0040, payload.len()) };
    if hglobal.is_null() {
        return Err("Memoria clipboard insufficiente".into());
    }
    let mut allocation = Allocation(hglobal);
    let ptr = unsafe { GlobalLock(hglobal) } as *mut u8;
    if ptr.is_null() {
        return Err("Impossibile preparare la clipboard".into());
    }
    unsafe {
        copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
        GlobalUnlock(hglobal);
    }
    // Clipboard's new owner must be a real window, not NULL.
    let owner = unsafe { GetForegroundWindow() };
    if owner.is_null() {
        return Err("Apri Veldir in primo piano per esportare i file".into());
    }
    if unsafe { OpenClipboard(owner) } == 0 {
        return Err("Clipboard occupata: riprova".into());
    }
    let _clipboard = ClipboardGuard;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("Impossibile svuotare la clipboard".into());
    }
    if unsafe { SetClipboardData(15, hglobal) }.is_null() {
        return Err("Impossibile pubblicare i percorsi nella clipboard".into());
    }
    // From this point on, Windows owns and frees the allocation.
    allocation.0 = null_mut();
    Ok(())
}

#[cfg(not(windows))]
pub fn copy_files_to_explorer(_paths: &[PathBuf]) -> Result<(), String> {
    Err("L'esportazione file nella clipboard è disponibile solo su Windows".into())
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;
    #[test]
    fn utf16_dropfiles_are_accepted_by_native_windows_drag_query_file() {
        use std::ffi::{OsString, c_void};
        use std::os::windows::ffi::OsStringExt;
        use std::ptr::{copy_nonoverlapping, null_mut};

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
            fn GlobalLock(handle: *mut c_void) -> *mut c_void;
            fn GlobalUnlock(handle: *mut c_void) -> i32;
            fn GlobalFree(handle: *mut c_void) -> *mut c_void;
        }
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn DragQueryFileW(hdrop: *mut c_void, index: u32, buf: *mut u16, chars: u32) -> u32;
        }

        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("report - à - 漢字.txt");
        let second = directory.path().join("second file.txt");
        std::fs::write(&first, "one").unwrap();
        std::fs::write(&second, "two").unwrap();

        let data = encode_dropfiles(&[first.clone(), second.clone()]).unwrap();
        assert_eq!(&data[..4], &20u32.to_le_bytes());
        assert_eq!(&data[16..20], &1u32.to_le_bytes());
        assert_eq!(&data[data.len() - 4..], &[0, 0, 0, 0]);

        // Use a movable HGLOBAL just like SetClipboardData would, but do NOT
        // open or mutate the desktop clipboard while running automated tests.
        let handle = unsafe { GlobalAlloc(0x0002 | 0x0040, data.len()) };
        assert!(!handle.is_null());
        let locked = unsafe { GlobalLock(handle) };
        assert!(!locked.is_null());
        unsafe { copy_nonoverlapping(data.as_ptr(), locked.cast(), data.len()) };

        let count = unsafe { DragQueryFileW(locked, u32::MAX, null_mut(), 0) };
        let mut decoded = Vec::new();
        for index in 0..count {
            let size = unsafe { DragQueryFileW(locked, index, null_mut(), 0) };
            let mut buffer = vec![0u16; size as usize + 1];
            let written =
                unsafe { DragQueryFileW(locked, index, buffer.as_mut_ptr(), size + 1) };
            decoded.push(PathBuf::from(OsString::from_wide(&buffer[..written as usize])));
        }
        unsafe {
            GlobalUnlock(handle);
            GlobalFree(handle);
        }
        assert_eq!(count, 2);
        assert_eq!(decoded, vec![first.canonicalize().unwrap(), second.canonicalize().unwrap()]);
        assert_eq!(std::fs::read_to_string(first).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "two");
    }

    #[test]
    fn dropfiles_export_rejects_empty_missing_and_too_many_paths() {
        assert!(encode_dropfiles(&[]).is_err());
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.txt");
        assert!(encode_dropfiles(&[missing]).is_err());

        let existing = dir.path().join("real.txt");
        std::fs::write(&existing, "ok").unwrap();
        assert!(encode_dropfiles(&vec![existing; 501]).is_err());
    }

    #[test]
    fn export_rejects_empty_paths_without_modifying_clipboard() {
        assert!(copy_files_to_explorer(&[]).is_err());
    }
    #[test]
    fn export_rejects_missing_files_without_modifying_clipboard() {
        let path = std::env::temp_dir().join("veldir-missing-file-for-test-94f3d6a");
        assert!(copy_files_to_explorer(&[path]).is_err());
    }
}
