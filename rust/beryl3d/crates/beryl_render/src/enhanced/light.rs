//! The Enhanced tier's lights (docs/beryl-3d.md, "The Enhanced tier": "Per-pixel lighting"),
//! which `shade_card_lit` and `shade_table_lit` in `light.wgsl` shade with.
//!
//! Not tuned yet: the key light shines straight down and the fill light is off.

use beryl_scene::frame::Frame;
use glam::{Vec4, Vec4Swizzles};

/// The lights a frame is shaded with, in the table's space, where z is height.
pub(crate) struct Lighting {
    /// The direction toward the key light; w is unused.
    pub(crate) key: Vec4,
    /// The camera's position; w is unused.
    pub(crate) eye: Vec4,
    /// The fill light's color and strength.
    pub(crate) fill: Vec4,
}

/// The lights of `frame`.
pub(crate) fn lighting(frame: &Frame) -> Lighting {
    Lighting {
        key: Vec4::Z,
        eye: eye(frame).extend(1.0),
        fill: Vec4::ZERO,
    }
}

/// The camera's position in the table's space, from the inverse of its view and projection: the
/// point a perspective camera projects to clip space's (0, 0, z, 0). An orthographic camera has
/// no such point, being infinitely far away, so its eye is the centre of its near plane.
fn eye(frame: &Frame) -> glam::Vec3 {
    let inverse = frame.view_projection.inverse();
    let eye = inverse * Vec4::Z;
    if eye.w.abs() > f32::EPSILON {
        return eye.xyz() / eye.w;
    }
    let near = inverse * Vec4::W;
    near.xyz() / near.w
}
