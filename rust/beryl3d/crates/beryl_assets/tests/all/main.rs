//! The integration tests of `beryl_assets`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

mod faces;
mod frame;
mod glyph_coverage;
mod glyphs;
mod icons;
mod light;
mod literal_glyphs;
mod packer;
