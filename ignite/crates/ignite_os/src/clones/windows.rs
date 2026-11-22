//! The Windows half of `clones`. NTFS makes no clones, so there every byte of a file is its
//! own. `ReFS`, which a Dev Drive uses, counts references to its blocks, and from Windows 11 24H2
//! `CopyFile` (which `std::fs::copy` calls) clones a file's blocks instead of writing them
//! again when source and target are on one such volume: a 984 MB copy on a Dev Drive took
//! 23 MB of free space on 2026-11-19.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};

use super::Clones;
use crate::Native;

/// The file system flag that says a volume counts references to its blocks, from
/// `Win32::System::SystemServices`, a feature this crate does not otherwise need.
const FILE_SUPPORTS_BLOCK_REFCOUNTING: u32 = 0x0800_0000;

impl Clones for Native {
    fn copies_are_clones(&self, path: &Path) -> bool {
        // Keep the filesystem capabilities for each volume that holds a tree.
        static VOLUMES: OnceLock<Mutex<HashMap<PathBuf, bool>>> = OnceLock::new();
        let Some(volume) = volume_of(path) else {
            return false;
        };
        let volumes = VOLUMES.get_or_init(Mutex::default);
        let mut volumes = volumes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *volumes
            .entry(volume.clone())
            .or_insert_with(|| counts_block_references(&volume))
    }

    /// Windows has no call that clones a directory hierarchy, so the caller copies it file by
    /// file.
    fn clone_tree(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// The root of the volume that holds `path`, such as `D:\`.
fn volume_of(path: &Path) -> Option<PathBuf> {
    let name = wide(path.as_os_str());
    let mut root = [0u16; 1024];
    // SAFETY: `name` is NUL-terminated and `root` is writable for the length passed; both
    // outlive the call.
    let ok = unsafe {
        GetVolumePathNameW(
            name.as_ptr(),
            root.as_mut_ptr(),
            u32::try_from(root.len()).expect("a short buffer"),
        )
    };
    if ok == 0 {
        return None;
    }
    let end = root.iter().position(|&c| c == 0).unwrap_or(root.len());
    Some(PathBuf::from(std::ffi::OsString::from_wide(&root[..end])))
}

/// Whether the volume whose root is `root` counts references to its blocks.
fn counts_block_references(root: &Path) -> bool {
    let name = wide(root.as_os_str());
    let mut flags = 0u32;
    // SAFETY: `name` is NUL-terminated and `flags` is writable; the buffers not wanted are null
    // with a length of zero, which the call accepts.
    let ok = unsafe {
        GetVolumeInformationW(
            name.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut flags,
            std::ptr::null_mut(),
            0,
        )
    };
    ok != 0 && flags & FILE_SUPPORTS_BLOCK_REFCOUNTING != 0
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}
