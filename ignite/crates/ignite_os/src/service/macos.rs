//! The macOS half of `service`.
//!
//! Stub. The ticket Thrh4.8 (macOS process, usage and socket layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::path::Path;

use super::Service;
use crate::Native;

impl Service for Native {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn uninstall(&self) -> io::Result<()> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn is_installed(&self) -> io::Result<bool> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }
}
