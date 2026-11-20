//! The integration tests of `beryl_render`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

mod bloom;
mod downlevel;
mod enhanced;
mod enhanced_boards;
mod faces;
mod light;
mod render;
mod shader_cost;
mod shadow;
mod tabletop;
mod text;
