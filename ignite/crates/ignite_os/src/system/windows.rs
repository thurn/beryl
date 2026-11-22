//! The Windows half of `system`: the size and free space of a disk from `GetDiskFreeSpaceExW`, and the
//! installed memory from `GlobalMemoryStatusEx`.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use super::DiskSpace;

/// The volume's size and the bytes the current user can still write.
pub fn disk_space(path: &Path) -> io::Result<DiskSpace> {
    let name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut total = 0;
    let mut available = 0;
    // SAFETY: `name` is NUL-terminated and outlives the call. Both output counts
    // are writable and the unneeded count is null, which the call accepts.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &raw mut available,
            &raw mut total,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(DiskSpace { total, available })
}

/// Local drive roots, including mounted Dev Drives with assigned letters.
pub fn mount_points() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

    // SAFETY: GetLogicalDrives takes no arguments.
    let drives = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|&index| drives & (1 << index) != 0)
        .filter_map(|index| {
            let letter = b'A' + index;
            let name = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
            // SAFETY: name is NUL-terminated and lives through the call. DRIVE_FIXED is 3.
            (unsafe { GetDriveTypeW(name.as_ptr()) } == 3)
                .then(|| PathBuf::from(format!("{}:\\", char::from(letter))))
        })
        .collect()
}

/// The machine's physical memory in bytes.
pub fn total_memory_bytes() -> io::Result<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).expect("a small record"),
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: `status` is a valid record whose `dwLength` gives its size, as the call needs.
    if unsafe { GlobalMemoryStatusEx(&raw mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.ullTotalPhys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temporary_directory_has_free_space_and_the_machine_has_memory() {
        let space = disk_space(&ignite_core::test_files::temp_dir()).unwrap();
        assert!(space.available > 0 && space.total >= space.available);
        assert!(total_memory_bytes().unwrap() > 1 << 30);
    }
}
