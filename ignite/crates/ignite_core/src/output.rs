//! Command output: every verb's text follows one contract. The first line is the verdict,
//! failures follow with a file, a line and the command that reruns each alone, and the whole is
//! at most 40 lines, with the full output in a file whose path is printed.
//!
//! Stub. The ticket Thrh4.7 (Command output and the workflow text) owns this file and replaces
//! the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

/// The most lines a command prints by default.
pub const MAX_LINES: usize = 40;

/// One labelled line, such as `landed   3a9e4c1 on staging`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub label: String,
    pub value: String,
}

/// Renders labelled lines in aligned columns, cut to [`MAX_LINES`] with a pointer to `full`.
pub fn render(lines: &[Line], full: Option<&Path>) -> String {
    todo!("Thrh4.7 (Command output and the workflow text)")
}
