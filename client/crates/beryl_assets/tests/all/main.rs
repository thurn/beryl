//! The integration tests of `beryl_assets`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

#[path = "../../../../test_scans.rs"]
mod test_scans;

mod faces;
mod frame;
mod glyphs;
mod light;
mod literal_glyphs;
mod packer;
