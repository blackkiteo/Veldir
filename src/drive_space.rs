//! Read-only available disk space for a Windows drive or mapped network share.
//! Never enumerate files recursively and never query disk space on the UI thread.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskSpace {
    /// Bytes this user/process can still allocate, respecting disk quotas.
    pub available: u64,
    /// Total bytes visible to this user, respecting volume quotas.
    pub capacity: u64,
}

impl DiskSpace {
    /// Avoid showing a misleading fraction if an exotic filesystem reports
    /// inconsistent values.
    pub fn valid(self) -> bool {
        self.available <= self.capacity && self.capacity > 0
    }
}

#[cfg(windows)]
pub fn query(path: &Path) -> Result<DiskSpace, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
    if name.is_empty() || name.contains(&0) {
        return Err("Percorso unità non valido".into());
    }
    name.push(0);
    let mut available = 0u64;
    let mut capacity = 0u64;
    let mut total_free = 0u64;
    let success = unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &mut available,
            &mut capacity,
            &mut total_free,
        )
    };
    if success == 0 {
        return Err(format!(
            "Unità non disponibile o spazio non leggibile: {}",
            std::io::Error::last_os_error()
        ));
    }
    let space = DiskSpace { available, capacity };
    if !space.valid() {
        return Err("L'unità ha restituito una capacità non valida".into());
    }
    Ok(space)
}

#[cfg(not(windows))]
pub fn query(_path: &Path) -> Result<DiskSpace, String> {
    Err("Lo spazio unità è disponibile solo su Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_aware_free_space_cannot_exceed_accessible_capacity() {
        assert!(DiskSpace { available: 0, capacity: 4096 }.valid());
        assert!(DiskSpace { available: 4096, capacity: 4096 }.valid());
        assert!(!DiskSpace { available: 4097, capacity: 4096 }.valid());
        assert!(!DiskSpace { available: 0, capacity: 0 }.valid());
    }

    #[cfg(windows)]
    #[test]
    fn windows_temp_volume_has_readable_space_without_modifying_files() {
        let temp = std::env::temp_dir();
        let space = query(&temp).expect("Windows temp volume should be accessible");
        assert!(space.valid());
    }
}
