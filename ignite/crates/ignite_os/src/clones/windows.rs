//! The Windows half of `clones`: NTFS makes no clones, so every byte of a file is its own.

use std::io;
use std::path::Path;

use super::{Clones, Sharing};
use crate::Native;

impl Clones for Native {
    fn sharing(&self, _path: &Path) -> io::Result<Option<Sharing>> {
        Ok(None)
    }
}
