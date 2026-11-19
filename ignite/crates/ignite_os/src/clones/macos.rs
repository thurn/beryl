//! The macOS half of `clones`: `getattrlist` with the extended common attributes
//! `ATTR_CMNEXT_PRIVATESIZE` (the bytes no clone or snapshot shares) and `ATTR_CMNEXT_CLONEID`.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Clones, Sharing};
use crate::Native;

impl Clones for Native {
    fn sharing(&self, path: &Path) -> io::Result<Option<Sharing>> {
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("a path holds a NUL byte"))?;
        // SAFETY: an all-zero `attrlist` is a valid request for no attributes.
        let mut request: libc::attrlist = unsafe { std::mem::zeroed() };
        request.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
        // With `FSOPT_ATTR_CMN_EXTENDED`, the fork attributes' field holds the extended common
        // attributes.
        request.forkattr = libc::ATTR_CMNEXT_PRIVATESIZE | libc::ATTR_CMNEXT_CLONEID;
        // The reply: its length (u32), then the attributes in the order of their bits, an
        // `off_t` and a `u64`, packed at 4-byte alignment.
        let mut reply = [0u8; 4 + 8 + 8];
        // SAFETY: `name` is NUL-terminated, `request` is a valid `attrlist`, and `reply` is
        // writable for the length passed; all three outlive the call.
        let status = unsafe {
            libc::getattrlist(
                name.as_ptr(),
                (&raw mut request).cast(),
                reply.as_mut_ptr().cast(),
                reply.len(),
                libc::FSOPT_NOFOLLOW | libc::FSOPT_ATTR_CMN_EXTENDED,
            )
        };
        if status != 0 {
            let error = io::Error::last_os_error();
            // A file system that does not know the extended attributes (an older HFS+ volume or
            // a network share) refuses the request.
            return match error.raw_os_error() {
                Some(libc::EINVAL | libc::ENOTSUP) => Ok(None),
                _ => Err(error),
            };
        }
        let length = u32::from_ne_bytes(reply[0..4].try_into().expect("four bytes"));
        if (length as usize) < reply.len() {
            return Ok(None);
        }
        let private = i64::from_ne_bytes(reply[4..12].try_into().expect("eight bytes"));
        let clone_id = u64::from_ne_bytes(reply[12..20].try_into().expect("eight bytes"));
        Ok(Some(Sharing {
            private: u64::try_from(private).unwrap_or(0),
            clone_id: Some(clone_id),
        }))
    }

    /// The startup disk is APFS on every Mac Ignite runs on, and `std::fs::copy` clones there
    /// (it calls `fclonefileat` first).
    fn copies_are_clones(&self, _path: &Path) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    /// A clone shares all of its blocks with its source and has its clone ID, and a plain copy
    /// owns its blocks. The temporary directory is on the startup disk, which is APFS.
    #[test]
    fn a_clone_owns_none_of_its_blocks_and_a_copy_owns_all_of_them() {
        let dir = std::env::temp_dir().join(format!("ignite-clones-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source");
        let bytes: Vec<u8> = (0..1_000_000u32).map(|n| (n * 7 % 251) as u8).collect();
        fs::write(&source, &bytes).unwrap();
        // `cp -c` clones; `cp` without it writes every block again.
        for (flag, name) in [(Some("-c"), "clone"), (None, "copy")] {
            let mut cp = Command::new("cp");
            cp.args(flag).arg(&source).arg(dir.join(name));
            assert!(cp.status().unwrap().success());
        }

        let of = |name: &str| Native.sharing(&dir.join(name)).unwrap().unwrap();
        let (source, clone, copy) = (of("source"), of("clone"), of("copy"));
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(clone.private, 0);
        assert_eq!(source.private, 0);
        assert_eq!(clone.clone_id, source.clone_id);
        assert!(copy.private >= 1_000_000, "{copy:?}");
        assert_ne!(copy.clone_id, source.clone_id);
    }
}
