//! Procedural Enhanced-tier visual effects emitted by the toolkit: combat chevrons, blocking
//! shield domes, and deterministic looping ember sparks.

use std::f32::consts::TAU;

use glam::{Affine3A, Vec2, Vec3};
use kit_base::atlas::Tier;
use kit_base::data::light::{CombatSparks, Own};
use kit_base::tokens::color;
use kit_frame::frame::vfx::HighlightStyle;
use kit_frame::frame::{Band, Draw, FrameBuilder, Vfx, sprites, vfx};

/// IEEE-754 representation of `2^-24` (`1 / 16_777_216`), used to map a 24-bit integer hash
/// uniformly into `[0, 1)`.
const UNIT_24_BITS: u32 = 0x3380_0000;

/// Period over which continuous ambient VFX clocks wrap, in nanoseconds (1000 seconds),
/// keeping `f32` phase precision crisp across long games.
const LOOP_WRAP_NS: u64 = 1_000_000_000_000;

/// Inputs for emitting a state-driven looping procedural VFX on a card face or anchor.
#[derive(Clone, Copy, Debug)]
pub struct StateLoop {
    /// Procedural VFX kind to draw.
    pub kind: Vfx,
    /// Render band to draw into (typically `Band::Glows`).
    pub band: Band,
    /// Per-slot or per-anchor phase seed in `[0, 1)` (for example `unit_hash(slot, 0)`).
    pub seed: f32,
    /// Frame clock in nanoseconds, wrapped internally into seconds.
    pub now: u64,
    /// Quad height-to-width ratio (`size.y / size.x`).
    pub aspect: f32,
    /// RGBA tint passed to the shader.
    pub tint: [u8; 4],
    /// Combined opacity in `[0, 1]`.
    pub opacity: f32,
    /// When true, reduced motion is active: nothing is drawn and ambient wake is not requested.
    pub reduced_motion: bool,
}

/// Converts a nanosecond frame clock into a wrapped looping time in seconds (`[0, 1000)`).
#[must_use]
pub fn loop_time_s(now: u64) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "wrapped to 1000 seconds so f32 retains sub-millisecond precision"
    )]
    let time_s = (now % LOOP_WRAP_NS) as f32 / 1e9;
    time_s
}

/// Writes a state-driven looping procedural VFX quad on a card face or anchor as a pure function
/// of `(seed, now)`.
///
/// Draws only in the Enhanced tier and does nothing under `reduced_motion` or zero opacity.
/// Pushing a `Material::Vfx` draw sets `Frame::ambient_motion`, which keeps live hosts ticking
/// through `display_wake` while finite scripts and `--do still` still settle (`Wake::Never`).
pub fn write_state_loop(builder: &mut FrameBuilder<'_>, transform: Affine3A, fx: &StateLoop) {
    if builder.tier() != Tier::Enhanced || fx.reduced_motion || fx.opacity <= 0.0 {
        return;
    }
    let time_s = loop_time_s(fx.now);
    builder.draw(
        fx.band,
        Draw::vfx(
            transform,
            fx.tint,
            fx.kind,
            [time_s, fx.opacity.clamp(0.0, 1.0), fx.seed, fx.aspect],
        ),
    );
}

/// Inputs for emitting an Enhanced-tier procedural highlight shimmer loop and one-shot appear pop
/// around a rectangular subject.
#[derive(Clone, Copy, Debug)]
pub struct HighlightVfx {
    /// Highlight palette and motion style (`Eligible`, `Selected`, or `Hover`).
    pub style: HighlightStyle,
    /// Render band for the looping outline shimmer (matches the subject's mark band).
    pub band: Band,
    /// Render band for the one-shot 400 ms appear pop (typically `Band::Glows` on the table so it
    /// receives calibrated bloom, or `band` inside flat layers).
    pub pop_band: Band,
    /// Scissor clip index inherited from the subject.
    pub clip: u8,
    /// Per-subject phase seed in `[0, 1)`.
    pub seed: f32,
    /// Frame clock in nanoseconds.
    pub now: u64,
    /// Nanoseconds elapsed since the highlight began appearing.
    pub age_ns: u64,
    /// Subject box width and height in local units before scaling.
    pub base_size: Vec2,
    /// RGBA tint including any entrance or exit fade in its alpha channel.
    pub tint: [u8; 4],
    /// When true, reduced motion is active: procedural motion is skipped so the caller can fall
    /// back to a static outline without requesting ambient wake.
    pub reduced_motion: bool,
}

/// Writes an Enhanced-tier looping highlight shimmer and, during the first 400 ms after appear, a
/// one-shot outward appear pop.
///
/// Returns `true` when the Enhanced procedural highlight was drawn, or `false` in the Baseline
/// tier or under `reduced_motion` so the caller can draw the Baseline outline instead.
pub fn write_highlight(
    builder: &mut FrameBuilder<'_>,
    fx: &HighlightVfx,
    place: impl Fn(Vec2) -> Affine3A,
) -> bool {
    if builder.tier() != Tier::Enhanced || fx.reduced_motion {
        return false;
    }
    if fx.tint[3] == 0 {
        return true;
    }
    let shimmer_size =
        fx.base_size + Vec2::splat(2.0 * vfx::HIGHLIGHT_SHIMMER_PAD * fx.base_size.x);
    let shimmer_aspect = shimmer_size.y / shimmer_size.x.max(f32::EPSILON);
    builder.draw(
        fx.band,
        Draw::vfx_styled(
            place(shimmer_size),
            fx.tint,
            Vfx::HighlightShimmer,
            fx.style as u8,
            [loop_time_s(fx.now), 1.0, fx.seed, shimmer_aspect],
        )
        .with_clip(fx.clip),
    );
    if fx.age_ns < vfx::HIGHLIGHT_POP_NS {
        #[expect(
            clippy::cast_precision_loss,
            reason = "age_ns is strictly below 400_000_000 ns"
        )]
        let pop_t = fx.age_ns as f32 / vfx::HIGHLIGHT_POP_NS as f32;
        let pop_size = fx.base_size + Vec2::splat(2.0 * vfx::HIGHLIGHT_POP_PAD * fx.base_size.x);
        let pop_aspect = pop_size.y / pop_size.x.max(f32::EPSILON);
        builder.draw(
            fx.pop_band,
            Draw::vfx_styled(
                place(pop_size),
                fx.tint,
                Vfx::HighlightPop,
                fx.style as u8,
                [pop_t, 1.0, fx.seed, pop_aspect],
            )
            .with_clip(fx.clip),
        );
    }
    true
}

/// Computes a cheap Baseline-tier animated tint for an outline highlight: a slow 3.5 s shimmer
/// plus a brief 400 ms brightness lift when the highlight first appears.
#[must_use]
pub fn baseline_highlight_tint(tint: [u8; 4], now: u64, age_ns: u64, seed: f32) -> [u8; 4] {
    let time_s = loop_time_s(now);
    let wave = 0.5 + 0.5 * (time_s * (TAU / 3.5) + seed * TAU).sin();
    let pop = if age_ns < vfx::HIGHLIGHT_POP_NS {
        #[expect(
            clippy::cast_precision_loss,
            reason = "age_ns is strictly below 400_000_000 ns"
        )]
        let u = 1.0 - age_ns as f32 / vfx::HIGHLIGHT_POP_NS as f32;
        u * u
    } else {
        0.0
    };
    let lift = 0.14 * wave + 0.30 * pop;
    let alpha_scale = (0.90 + 0.10 * wave + 0.08 * pop).min(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 255] before casting"
    )]
    let lift_channel = |channel: u8, target: f32| -> u8 {
        let c = f32::from(channel);
        (c + (target - c) * lift).clamp(0.0, 255.0).round() as u8
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 255] before casting"
    )]
    let alpha = (f32::from(tint[3]) * alpha_scale).clamp(0.0, 255.0).round() as u8;
    [
        lift_channel(tint[0], 195.0),
        lift_channel(tint[1], 255.0),
        lift_channel(tint[2], 242.0),
        alpha,
    ]
}

/// Which combat visual a fighting subject wears in the Enhanced tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FighterKind {
    Attacker { blocked: bool },
    Blocker,
}

/// Inputs for emitting a fighting subject's Enhanced-tier procedural VFX and ember sparks.
#[derive(Clone, Copy, Debug)]
pub struct FighterVfx {
    pub slot: usize,
    pub kind: FighterKind,
    pub glow_band: Band,
    pub particle_band: Band,
    /// Width of the card edge facing the opposing side, in millimetres.
    pub across: f32,
    /// Milliseconds since the subject entered its current fighting state.
    pub age_ms: f32,
    /// Wrapped animation clock in seconds.
    pub time_s: f32,
    /// Combined card and entrance-fade opacity in `[0, 1]`.
    pub opacity: f32,
}

/// Deterministic fractional hash in `[0, 1)` for per-card VFX phase seeds and spark emitters.
#[must_use]
pub fn unit_hash(slot: usize, index: u32) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "card slots fit u32 and only their bits feed the hash"
    )]
    let mut x = (slot as u32)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(index.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    #[expect(
        clippy::cast_precision_loss,
        reason = "24-bit integer fits an f32 significand exactly"
    )]
    let mantissa = (x & 0x00FF_FFFF) as f32;
    mantissa * f32::from_bits(UNIT_24_BITS)
}

/// Writes an Enhanced-tier procedural attacker chevron (unblocked crimson-ember or blocked
/// solar-gold) with looping ember sparks, or a golden blocker shield dome.
///
/// `place(out, size)` returns the world transform of a quad `size` large positioned `out`
/// millimetres past the facing card edge.
pub fn write_fighter(
    builder: &mut FrameBuilder<'_>,
    own: &Own,
    fx: &FighterVfx,
    place: impl Fn(f32, Vec2) -> Affine3A,
) {
    let card_width = builder.card().x;
    let seed = unit_hash(fx.slot, 0);
    match fx.kind {
        FighterKind::Attacker { blocked } => {
            let tuning = &own.combat_fx.attacker;
            let pop = (-fx.age_ms / own.wedge.pop_ms).exp();
            let base_width = fx.across * tuning.width_scale * (1.0 + own.wedge.pop.x * pop);
            let min_h = card_width * tuning.height_range.x;
            let max_h = card_width * tuning.height_range.y;
            let height = (fx.across * tuning.height_scale).clamp(min_h, max_h)
                * (1.0 + own.wedge.pop.y * pop);
            let (size, out) = vfx::padded_quad(base_width, height, vfx::CHEVRON_PAD);
            let kind = if blocked {
                Vfx::ChevronBlocked
            } else {
                Vfx::Chevron
            };
            builder.draw(
                fx.glow_band,
                Draw::vfx(
                    place(out, size),
                    [u8::MAX; 4],
                    kind,
                    [fx.time_s, fx.opacity, seed, height / base_width],
                ),
            );
            write_attacker_sparks(
                builder,
                &tuning.sparks,
                fx,
                (base_width, height, card_width),
                blocked,
                &place,
            );
        }
        FighterKind::Blocker => {
            let tuning = &own.combat_fx.blocker;
            let t = (fx.age_ms / own.crescent.open_ms).clamp(0.0, 1.0);
            let open = own.crescent.opens_from
                + (1.0 - own.crescent.opens_from) * (1.0 - (1.0 - t).powi(3));
            let span_width = fx.across * tuning.width_scale * open;
            let height =
                (card_width * tuning.height_card * open).min(fx.across * tuning.height_edge_max);
            let (size, out) = vfx::padded_quad(span_width, height, vfx::SHIELD_PAD);
            builder.draw(
                fx.glow_band,
                Draw::vfx(
                    place(out, size),
                    [u8::MAX; 4],
                    Vfx::ShieldDome,
                    [fx.time_s, fx.opacity, seed, height / span_width],
                ),
            );
        }
    }
}

/// Emits deterministic looping ember sparks drifting outward from an attacker's chevron edges,
/// respecting the frame's shared particle ceiling.
fn write_attacker_sparks(
    builder: &mut FrameBuilder<'_>,
    sparks: &CombatSparks,
    fx: &FighterVfx,
    (base_width, height, card_width): (f32, f32, f32),
    blocked: bool,
    place: &impl Fn(f32, Vec2) -> Affine3A,
) {
    let wanted = if blocked {
        sparks.blocked_count
    } else {
        sparks.unblocked_count
    };
    let room = u32::try_from(builder.particle_room()).unwrap_or(u32::MAX);
    let count = wanted.min(room);
    let base_tint = if blocked {
        color::COMBAT_SPARK_BLOCKED
    } else {
        color::COMBAT_SPARK
    };
    for index in 0..count {
        let h0 = unit_hash(fx.slot, 1 + index * 3);
        let h1 = unit_hash(fx.slot, 2 + index * 3);
        let h2 = unit_hash(fx.slot, 3 + index * 3);
        let cycle = sparks.cycle_s.x + sparks.cycle_s.y * h0;
        let u = (fx.time_s / cycle + h1).fract();
        let side = if index % 2 == 0 { -1.0 } else { 1.0 };
        let along = sparks.along.x + sparks.along.y * h2;
        let start_lat = side * sparks.edge_inset * base_width * (1.0 - along);
        let start_out = height * along;
        let sway =
            (u * TAU * sparks.sway_turns + h0 * TAU).sin() * (card_width * sparks.sway_reach) * u;
        let lat = start_lat + sway;
        let out = start_out + u * card_width * (sparks.drift.x + sparks.drift.y * h1);
        let radius =
            card_width * (sparks.radius.x + sparks.radius.y * h2) * (1.0 - sparks.shrink * u);
        let envelope = (u * sparks.fade_in).min(1.0) * (1.0 - u);
        let spark_tint = with_opacity(base_tint, fx.opacity * envelope);
        let origin = place(out, Vec2::splat(radius));
        let shifted = origin
            * Affine3A::from_translation(Vec3::new(lat / radius.max(f32::EPSILON), 0.0, 0.0));
        builder.draw(
            fx.particle_band,
            Draw::particle(shifted, spark_tint, sprites::DISC, u, h0),
        );
    }
}

fn with_opacity([red, green, blue, alpha]: [u8; 4], opacity: f32) -> [u8; 4] {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a byte, clamped to 0 to 255"
    )]
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}

#[cfg(test)]
mod tests {
    use super::{UNIT_24_BITS, unit_hash};

    #[test]
    fn unit_hash_is_deterministic_and_in_unit_interval() {
        assert!((f32::from_bits(UNIT_24_BITS) - 1.0 / 16_777_216.0).abs() < f32::EPSILON);
        for slot in [0, 1, 7, 42, 255] {
            for index in 0..16 {
                let value = unit_hash(slot, index);
                assert!((0.0..1.0).contains(&value), "{slot}/{index} -> {value}");
            }
        }
        assert_ne!(unit_hash(0, 0).to_bits(), unit_hash(1, 0).to_bits());
    }
}
