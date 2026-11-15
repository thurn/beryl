//! Beryl 3D's renderer: draws a frame's draw list with wgpu, in the Baseline tier
//! (docs/beryl-3d.md, "Rendering"). It is the only crate that names `wgpu`.

mod atlas;
mod count;
mod faces;
mod mesh;
mod pipeline;
mod renderer;
mod runs;
mod surface;
mod target;
mod textures;

pub use atlas::{GLOW_SPRITE, GLYPH_PAGES};
pub use mesh::CARD_RADIUS;
pub use renderer::{Drawn, Limits, Renderer};
pub use surface::{Native, Presented, Surface};
pub use target::{Target, write_rgba_png};
pub use textures::BACK_LAYER;
