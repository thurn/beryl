//! Procedural Enhanced-tier visual effects (`Material::Vfx`) and shared quad padding geometry.
//!
//! The renderer's procedural VFX shader (`beryl_render::enhanced::vfx`) reads `Vfx` from
//! `Draw::uv[0]` and uses the same padding ratios so outer halos fade out before reaching the
//! quad border.

use glam::Vec2;

/// Which procedural effect `Material::Vfx` draws, stored in `Draw::uv[0]`.
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vfx {
    /// Unblocked attacking chevron: crimson-ember plasma body with luminous V-rim.
    Chevron = 0,
    /// Blocked attacking chevron: amber-to-gold plasma body with warm ivory V-rim.
    ChevronBlocked = 1,
    /// Blocking shield dome: polar-swirling golden forcefield curtain and crest ridge.
    ShieldDome = 2,
}

/// `[side, bottom, top]` quad padding around a chevron (`Vfx::Chevron`, `Vfx::ChevronBlocked`).
pub const CHEVRON_PAD: [f32; 3] = [0.20, 0.05, 0.24];
/// `[side, bottom, top]` quad padding around a shield dome (`Vfx::ShieldDome`).
pub const SHIELD_PAD: [f32; 3] = [0.14, 0.06, 0.20];

/// Returns `(quad_size, outward_center_offset)` for a procedural shape of `base_width` and
/// `height` with `[side, bottom, top]` padding ratios.
#[must_use]
pub fn padded_quad(base_width: f32, height: f32, [side, bottom, top]: [f32; 3]) -> (Vec2, f32) {
    let width = base_width * (1.0 + 2.0 * side);
    let quad_height = height + (bottom + top) * base_width;
    let center = f32::midpoint(height, (top - bottom) * base_width);
    (Vec2::new(width, quad_height), center)
}
