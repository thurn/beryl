//! The Enhanced tier's lights (docs/beryl-3d.md, "The Enhanced tier": "Per-pixel lighting"),
//! which `shade_card_lit` and `shade_table_lit` in `light.wgsl` shade with: an ambient term,
//! the key light, which casts the shadow map's shadows, a soft fill from the side opposite the
//! key, and a faint sheen. Their strengths are `enhanced.light` in `data/beryl3d/space.json`.
//!
//! The ambient term is not stored there but computed, so that it cannot drift from the rule it
//! follows: a card lying flat in the open, lit by the key and the fill, is drawn in its Baseline
//! color, `ambient + key * up·key_direction + fill * up·fill_direction == 1`. The fill reaches a
//! flat card as well as the key does, so it is part of the rule.

use beryl_scene::data::space::{enhanced::light, shadow};
use beryl_scene::frame::Frame;
use glam::{Vec2, Vec3, Vec4, Vec4Swizzles};

/// The lights a frame is shaded with, in the table's space, where z is height. Each light's
/// strength is folded into its direction's length, which leaves the w components free for the
/// numbers `light.wgsl` needs besides.
pub(crate) struct Lighting {
    /// The direction toward the key light times its strength, and the ambient term in w.
    pub(crate) key: Vec4,
    /// The camera's position, and the sheen's strength in w.
    pub(crate) eye: Vec4,
    /// The direction toward the fill light times its strength, and the sheen's power in w.
    pub(crate) fill: Vec4,
}

/// The lights of `frame`.
pub(crate) fn lighting(frame: &Frame) -> Lighting {
    let key = key_direction();
    let fill = fill_direction(key);
    let ambient = 1.0 - light::KEY * key.z - light::FILL * fill.z;
    Lighting {
        key: (key * light::KEY).extend(ambient),
        eye: eye(frame).extend(light::SHEEN),
        fill: (fill * light::FILL).extend(light::SHEEN_POWER),
    }
}

/// The unit direction toward the key light: the direction along which the Baseline tier's
/// contact shadows fall (`shadow.light` in `data/beryl3d/space.json`, which `cast` in
/// `beryl_scene`'s `table/shadows.rs` applies), reversed, so that the lit side of a card and its
/// shadow agree. A shadow falls `(light.x, -light.y)` millimetres across the table for each
/// millimetre of height, so the light is up and the other way.
fn key_direction() -> Vec3 {
    Vec3::new(-shadow::LIGHT.x, shadow::LIGHT.y, 1.0).normalize()
}

/// The unit direction toward the fill light: from the side of the table opposite the key,
/// `fill_elevation` degrees above it. A key straight overhead has no side, so the fill then comes
/// from the viewer's side, down the table's y.
fn fill_direction(key: Vec3) -> Vec3 {
    let across = (-key.truncate()).try_normalize().unwrap_or(Vec2::NEG_Y);
    let (rise, run) = light::FILL_ELEVATION.to_radians().sin_cos();
    (across * run).extend(rise)
}

/// The camera's position in the table's space, from the inverse of its view and projection: the
/// point a perspective camera projects to clip space's (0, 0, z, 0). An orthographic camera has
/// no such point, being infinitely far away, so its eye is the centre of its near plane.
fn eye(frame: &Frame) -> Vec3 {
    let inverse = frame.view_projection.inverse();
    let eye = inverse * Vec4::Z;
    if eye.w.abs() > f32::EPSILON {
        return eye.xyz() / eye.w;
    }
    let near = inverse * Vec4::W;
    near.xyz() / near.w
}
