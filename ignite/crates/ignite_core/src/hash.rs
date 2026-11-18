//! Input hashes: SHA-256 over a check's sorted inputs, its family's commands, the toolchain
//! version and, for a check that is not portable, the operating system.
//!
//! Stub. The ticket Thrh4.3 (Input resolution and hashing) owns this file and replaces the
//! `todo!()` bodies.
#![allow(unused_variables)]

use crate::config::Os;
use crate::ids::{InputHash, Oid};

/// Everything that goes into one check's input hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashParts<'a> {
    /// Each input path with its git blob or tree ID.
    pub inputs: &'a [(String, Oid)],
    /// The family's `list` and `run` commands and its products' build commands.
    pub commands: &'a [String],
    pub toolchain: &'a str,
    /// `None` for a check with `platforms = "any"`.
    pub platform: Option<Os>,
}

pub fn input_hash(parts: &HashParts<'_>) -> InputHash {
    todo!("Thrh4.3 (Input resolution and hashing)")
}
