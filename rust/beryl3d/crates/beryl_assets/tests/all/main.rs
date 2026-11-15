//! The integration tests of `beryl_assets`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

mod faces;
mod glyphs;
mod packer;
