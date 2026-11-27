//! Procedural Enhanced-tier visual effects (`Material::Vfx`) and shared quad padding geometry.
//!
//! The renderer's procedural VFX shader (`beryl_render::enhanced::vfx`) reads `Vfx` from
//! `Draw::uv[0]` and uses the same padding ratios so outer halos fade out before reaching the
//! quad border.

use glam::{Affine3A, Vec2};
pub use kit_base::data::effect::{HighlightStyle, Vfx};

use super::draw::{Draw, Material, Mesh, TextureArray};

/// Visual palette and motion style for procedural Enhanced lines (`Vfx::LineRibbon` and
/// `Vfx::LineHead`), stored in the low byte of `Draw::uv[3]`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    /// Electric cyan-blue live aiming arrow.
    Aim = 0,
    /// Vivid emerald-green friendly target arrow.
    FriendlyTarget = 1,
    /// Vivid crimson-red hostile target arrow.
    HostileTarget = 2,
    /// Slender rose-crimson confirmed attack beam.
    Attack = 3,
    /// Slender solar-gold confirmed block beam.
    Block = 4,
    /// Faint pale teal-silver connection line linking a source to its derived entry.
    Source = 5,
}

/// `[side, bottom, top]` quad padding around a chevron (`Vfx::Chevron`, `Vfx::ChevronBlocked`).
pub const CHEVRON_PAD: [f32; 3] = [0.20, 0.05, 0.24];
/// `[side, bottom, top]` quad padding around a shield dome (`Vfx::ShieldDome`).
pub const SHIELD_PAD: [f32; 3] = [0.14, 0.06, 0.20];
/// Length-to-width ratio of the faceted spearhead (`Vfx::LineHead`).
pub const LINE_HEAD_ASPECT: f32 = 2.16;
/// `[length_pad, width_pad]` quad padding around a spearhead's half-extents so its outer glow
/// fades cleanly before reaching the quad boundary. Must match `line_fx.wgsl`.
pub const LINE_HEAD_PAD: [f32; 2] = [0.55, 0.95];
/// Quad padding ratio (fraction of base width added to each side) around a highlight shimmer
/// (`Vfx::HighlightShimmer`). Must match `fx/highlight.wgsl`.
pub const HIGHLIGHT_SHIMMER_PAD: f32 = 0.14;
/// Quad padding ratio (fraction of base width added to each side) around a highlight appear pop
/// (`Vfx::HighlightPop`). Must match `fx/highlight.wgsl`.
pub const HIGHLIGHT_POP_PAD: f32 = 0.19;
/// Duration of the one-shot highlight appear pop, in nanoseconds (400 ms).
pub const HIGHLIGHT_POP_NS: u64 = 400_000_000;

/// Packs start and end ribbon width tapers (each in `(0, 1]`) into one `u16` (`start` in the low
/// byte, `end` in the high byte) for `vs_vfx`.
#[must_use]
pub fn pack_taper(start: f32, end: f32) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [1, 255] first"
    )]
    let byte = |v: f32| (v.clamp(1.0 / 255.0, 1.0) * 255.0).round() as u16;
    byte(start) | (byte(end) << 8)
}

/// Packs start and end mitre slants (each in `[-1, 1]`) into one `u16` (`start` in the low byte,
/// `end` in the high byte) for `vs_vfx`.
#[must_use]
pub fn pack_slant(start: f32, end: f32) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 255] first"
    )]
    let byte = |v: f32| ((v.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u16;
    byte(start) | (byte(end) << 8)
}

fn alpha_byte(opacity: f32) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 255] first"
    )]
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    alpha
}

impl Draw {
    /// A procedural Enhanced-tier visual effect of `kind` with effect-specific `params`.
    /// For combat shapes (`Vfx::Chevron`, `Vfx::ChevronBlocked`, `Vfx::ShieldDome`), `params`
    /// is `[time_s, fade, seed, aspect]`.
    #[must_use]
    pub fn vfx(transform: Affine3A, tint: [u8; 4], kind: Vfx, params: [f32; 4]) -> Self {
        Self::vfx_styled(transform, tint, kind, 0, params)
    }

    /// A procedural Enhanced-tier visual effect of `kind` and `style` byte (`Draw::uv[3]`)
    /// with effect-specific `params`.
    #[must_use]
    pub fn vfx_styled(
        transform: Affine3A,
        tint: [u8; 4],
        kind: Vfx,
        style: u8,
        params: [f32; 4],
    ) -> Self {
        let mut draw = Self::new(
            transform,
            tint,
            Material::Vfx,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        draw.uv = [kind as u16, 0, 0, u16::from(style)];
        draw.params = params;
        draw
    }

    /// One tapered, mitred segment of a procedural Enhanced-tier luminous line ribbon
    /// (`Vfx::LineRibbon`). `params` is `[time_s, v_start, v_end, length_ratio]`.
    #[must_use]
    pub fn vfx_line_ribbon(
        transform: Affine3A,
        opacity: f32,
        style: LineStyle,
        taper: [f32; 2],
        slant: [f32; 2],
        params: [f32; 4],
    ) -> Self {
        let mut draw = Self::new(
            transform,
            [u8::MAX, u8::MAX, u8::MAX, alpha_byte(opacity)],
            Material::Vfx,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        draw.uv = [
            Vfx::LineRibbon as u16,
            pack_taper(taper[0], taper[1]),
            pack_slant(slant[0], slant[1]),
            u16::from(style as u8),
        ];
        draw.params = params;
        draw
    }

    /// A procedural Enhanced-tier faceted glowing spearhead (`Vfx::LineHead`) seated at the tip
    /// of a luminous line. `params` is `[time_s, fade, seed, aspect]`.
    #[must_use]
    pub fn vfx_line_head(
        transform: Affine3A,
        opacity: f32,
        style: LineStyle,
        params: [f32; 4],
    ) -> Self {
        let mut draw = Self::new(
            transform,
            [u8::MAX, u8::MAX, u8::MAX, alpha_byte(opacity)],
            Material::Vfx,
            TextureArray::Atlas,
            Mesh::Quad,
        );
        draw.uv = [Vfx::LineHead as u16, 0, 0, u16::from(style as u8)];
        draw.params = params;
        draw
    }
}

/// Returns `(quad_size, outward_center_offset)` for a procedural shape of `base_width` and
/// `height` with `[side, bottom, top]` padding ratios.
#[must_use]
pub fn padded_quad(base_width: f32, height: f32, [side, bottom, top]: [f32; 3]) -> (Vec2, f32) {
    let width = base_width * (1.0 + 2.0 * side);
    let quad_height = height + (bottom + top) * base_width;
    let center = f32::midpoint(height, (top - bottom) * base_width);
    (Vec2::new(width, quad_height), center)
}
