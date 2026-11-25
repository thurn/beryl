//! The macOS half of `jobserver`: a FIFO in the GNU make protocol, one byte per slot. The FIFO
//! is made with `mkfifo` and opened with `O_RDWR | O_NONBLOCK`. Holding both ends in one
//! descriptor keeps the pipe open while no other process has it open, a write never waits for a
//! reader, and a read of the empty pipe fails with [`io::ErrorKind::WouldBlock`] at once.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::{Jobservers, Slots};
use crate::Native;

/// The FIFO's name in the run directory.
const FIFO: &str = "jobserver";

impl Jobservers for Native {
    fn make_jobserver(&self, dir: &Path) -> io::Result<Slots> {
        let path = dir.join(FIFO);
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("a FIFO path holds a NUL byte"))?;
        // SAFETY: `name` is a NUL-terminated string that outlives the call.
        if unsafe { libc::mkfifo(name.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Slots {
            fifo: open(&path)?,
            auth: format!("fifo:{}", path.display()),
        })
    }

    fn open_jobserver(&self, auth: &str) -> io::Result<Slots> {
        let path = auth
            .strip_prefix("fifo:")
            .ok_or_else(|| io::Error::other(format!("not a FIFO jobserver: {auth}")))?;
        Ok(Slots {
            fifo: open(Path::new(path))?,
            auth: auth.to_owned(),
        })
    }

    fn put_slots(&self, slots: &mut Slots, count: u32) -> io::Result<()> {
        slots.fifo.write_all(&vec![b'+'; count as usize])
    }

    fn take_slots(&self, slots: &mut Slots, count: u32) -> io::Result<u32> {
        let mut buffer = vec![0; count as usize];
        let mut taken = 0;
        while taken < buffer.len() {
            match slots.fifo.read(&mut buffer[taken..]) {
                Ok(0) => break,
                Ok(read) => taken += read,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(taken as u32)
    }
}

/// Opens the FIFO at `path` for reading and writing without blocking.
fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots put come back when taken, and taking from the empty pipe returns at once instead of
    /// waiting for a writer.
    #[test]
    fn a_fifo_holds_slots_and_never_blocks() {
        let dir =
            ignite_core::test_files::temp_dir().join(format!("ignite-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut slots = Native.make_jobserver(&dir).unwrap();
        Native.put_slots(&mut slots, 3).unwrap();
        assert_eq!(Native.take_slots(&mut slots, 8).unwrap(), 3);
        assert_eq!(Native.take_slots(&mut slots, 8).unwrap(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
