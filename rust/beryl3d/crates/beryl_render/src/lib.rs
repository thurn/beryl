//! Beryl 3D's renderer: draws a frame's draw list with wgpu, in the Baseline tier
//! (docs/beryl-3d.md, "Rendering"). It is the only crate that names `wgpu`.

mod mesh;
mod pipeline;
mod renderer;
mod runs;
mod surface;
mod target;
mod textures;

pub use mesh::CARD_RADIUS;
pub use renderer::{Drawn, Limits, Renderer};
pub use surface::{Presented, Surface};
pub use target::Target;
pub use textures::{BACK_LAYER, GLOW_SPRITE};
