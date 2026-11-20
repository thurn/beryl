//! The macOS half of `fifo`: `mkfifo`, then an `open` with `O_RDWR | O_NONBLOCK`. Holding both
//! ends in one descriptor keeps the pipe open while no other process has it open.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::Fifos;
use crate::Native;

impl Fifos for Native {
    fn open_fifo(&self, path: &Path) -> io::Result<File> {
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("a FIFO path holds a NUL byte"))?;
        // SAFETY: `name` is a NUL-terminated string that outlives the call.
        if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, Read, Write};

    use super::*;

    /// Bytes written come back in a read, and a read of the empty pipe fails at once instead of
    /// waiting for a writer.
    #[test]
    fn a_fifo_holds_bytes_and_never_blocks() {
        let dir =
            ignite_core::test_files::temp_dir().join(format!("ignite-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut fifo = Native.open_fifo(&dir.join("fifo")).unwrap();
        fifo.write_all(b"+++").unwrap();
        let mut buffer = [0; 8];
        assert_eq!(fifo.read(&mut buffer).unwrap(), 3);
        assert_eq!(
            fifo.read(&mut buffer).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
