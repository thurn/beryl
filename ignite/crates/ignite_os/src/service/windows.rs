//! The Windows half of `service`.
//!
//! Stub. The ticket Thrh4.9 (Windows process, usage and pipe layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::path::Path;

use super::Service;
use crate::Native;

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn uninstall(&self) -> io::Result<()> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn is_installed(&self) -> io::Result<bool> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }
}
