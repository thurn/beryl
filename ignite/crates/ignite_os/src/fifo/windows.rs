//! The Windows half of `fifo`: Windows has no FIFOs, so a Cargo build there runs on the job
//! count its grant had when it started.

use std::fs::File;
use std::io;
use std::path::Path;

use super::Fifos;
use crate::Native;

impl Fifos for Native {
    fn open_fifo(&self, _path: &Path) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows has no FIFOs",
        ))
    }
}
