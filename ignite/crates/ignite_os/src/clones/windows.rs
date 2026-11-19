//! The Windows half of `clones`. NTFS makes no clones, so there every byte of a file is its
//! own. `ReFS`, which a Dev Drive uses, counts references to its blocks, and from Windows 11 24H2
//! `CopyFile` (which `std::fs::copy` calls) clones a file's blocks instead of writing them
//! again when source and target are on one such volume: a 984 MB copy on a Dev Drive took
//! 23 MB of free space on 2026-11-19.
//!
//! `ReFS` reports no per-file count of the blocks a file shares, as APFS does. So a file on a
//! block-cloning volume is reported as all shared, under an ID made from its name, length and
//! modification time. A seeded tree's copy keeps its donor file's name, length and time
//! (`trees/seed.rs` sets the time), and so gets the donor's ID until a build replaces it, which
//! gives it a new time and a new ID. The trees' measure then counts each copy's blocks once, and
//! a file no other tree holds counts once under an ID of its own.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};

use super::{Clones, Sharing};
use crate::Native;

/// The file system flag that says a volume counts references to its blocks, from
/// `Win32::System::SystemServices`, a feature this crate does not otherwise need.
const FILE_SUPPORTS_BLOCK_REFCOUNTING: u32 = 0x0800_0000;

impl Clones for Native {
    fn sharing(&self, path: &Path) -> io::Result<Option<Sharing>> {
        if !self.copies_are_clones(path) {
            return Ok(None);
        }
        let meta = std::fs::symlink_metadata(path)?;
        let mut hasher = DefaultHasher::new();
        path.file_name().hash(&mut hasher);
        meta.len().hash(&mut hasher);
        meta.modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        Ok(Some(Sharing {
            private: 0,
            clone_id: Some(hasher.finish()),
        }))
    }

    fn copies_are_clones(&self, path: &Path) -> bool {
        // A measure asks once for every file, so each volume's answer is kept.
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
