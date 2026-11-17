//! Beryl 3D's renderer: draws a frame's draw list with wgpu, in the Baseline tier or, where the
//! device allows it, the Enhanced tier (docs/beryl-3d.md, "Rendering"). It is the only crate
//! that names `wgpu`.

mod atlas;
mod count;
mod draw;
mod enhanced;
mod faces;
mod mesh;
mod pipeline;
mod renderer;
mod runs;
mod slots;
mod surface;
mod target;
mod textures;

pub use atlas::ATLAS_PAGES;
pub use enhanced::SHADER as ENHANCED_SHADER;
pub use mesh::CARD_RADIUS;
pub use renderer::{Drawn, Limits, Renderer};
pub use surface::{Native, Presented, Surface};
pub use target::{Picture, Readback, Target, write_rgba_png};
pub use textures::BACK_LAYER;
