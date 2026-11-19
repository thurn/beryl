//! The Windows half of `system`: the free space of a disk from `GetDiskFreeSpaceExW`, and the
//! installed memory from `GlobalMemoryStatusEx`.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// The bytes that this user may still write on the disk that holds `path`, as `df` reports
/// them as available.
pub fn disk_free_bytes(path: &Path) -> io::Result<u64> {
    let name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    // SAFETY: `name` is NUL-terminated and outlives the call, `available` is writable, and the
    // two counts not wanted are null, which the call accepts.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &raw mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
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
        assert!(disk_free_bytes(&std::env::temp_dir()).unwrap() > 0);
        assert!(total_memory_bytes().unwrap() > 1 << 30);
    }
}
