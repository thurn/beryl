//! Clone a directory and its contents on APFS.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::Clones;
use crate::Native;

/// `clonefile`'s flag that clones a symbolic link as a link instead of what it points at, from
/// `<sys/clonefile.h>`, which the `libc` crate does not define.
const CLONE_NOFOLLOW: u32 = 0x0001;

impl Clones for Native {
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

    use super::*;

    /// A cloned directory holds every file under it with its content, modification time and
    /// mode, and its links as links.
    #[test]
    fn a_cloned_tree_keeps_each_files_time_mode_and_links() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::time::{Duration, SystemTime};
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-clone-tree-{}", std::process::id()));
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
