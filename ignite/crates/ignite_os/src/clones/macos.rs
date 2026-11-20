//! The macOS half of `clones`: `getattrlist` with the extended common attributes
//! `ATTR_CMNEXT_PRIVATESIZE` (the bytes no clone or snapshot shares) and `ATTR_CMNEXT_CLONEID`.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::{Clones, Sharing};
use crate::Native;

/// `clonefile`'s flag that clones a symbolic link as a link instead of what it points at, from
/// `<sys/clonefile.h>`, which the `libc` crate does not define.
const CLONE_NOFOLLOW: u32 = 0x0001;

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

    /// `clonefile(2)` clones a directory hierarchy as if each entry were cloned on its own.
    fn clone_tree(&self, from: &Path, to: &Path) -> io::Result<()> {
        let c_path = |path: &Path| {
            CString::new(path.as_os_str().as_bytes())
                .map_err(|_| io::Error::other("a path holds a NUL byte"))
        };
        let (source, target) = (c_path(from)?, c_path(to)?);
        // SAFETY: both paths are NUL-terminated and outlive the call.
        let status = unsafe { libc::clonefile(source.as_ptr(), target.as_ptr(), CLONE_NOFOLLOW) };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
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

    /// A cloned directory holds every file under it with its content, modification time and
    /// mode, and its links as links.
    #[test]
    fn a_cloned_tree_keeps_each_files_time_mode_and_links() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("ignite-clone-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let from = dir.join("from");
        fs::create_dir_all(from.join("deep/er")).unwrap();
        let file = from.join("deep/er/lib.rlib");
        fs::write(&file, "compiled").unwrap();
        let earlier = SystemTime::now() - Duration::from_hours(24);
        fs::File::open(&file)
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        std::os::unix::fs::symlink("er/lib.rlib", from.join("deep/link")).unwrap();

        Native.clone_tree(&from, &dir.join("to")).unwrap();

        let copy = dir.join("to/deep/er/lib.rlib");
        let meta = fs::metadata(&copy).unwrap();
        assert_eq!(fs::read_to_string(&copy).unwrap(), "compiled");
        assert_eq!(meta.modified().unwrap(), earlier);
        assert_eq!(meta.permissions().mode() & 0o777, 0o444);
        let link = dir.join("to/deep/link");
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("er/lib.rlib"));
        assert!(
            Native.clone_tree(&from, &dir.join("to")).is_err(),
            "the target must not exist"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
