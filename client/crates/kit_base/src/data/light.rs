//! Tuning of arrows, lines, marks, combat emblems and the shapes of light they draw from.

use glam::Vec2;
use serde::Deserialize;

use super::tuning::{fraction, interior, nonnegative, positive, read, vector};

/// Tuning of arrows, lines, marks and the shapes of light: `light.json`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Light {
    #[serde(deserialize_with = "positive")]
    pub outline_grow: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lift: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub on_table_below: f32,
    #[serde(deserialize_with = "positive")]
    pub tap_size: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub paying_lift: f32,
    #[serde(deserialize_with = "vector")]
    pub player_half_pixels: Vec2,
    #[serde(deserialize_with = "nonnegative")]
    pub sag: f32,
    pub hang: Hang,
    pub bow: Bow,
    pub onto: Onto,
    pub sized: Sized,
    pub wide: Wide,
    pub arrow: Arrow,
    pub swell: Swell,
    pub beam: Beam,
    pub glow: Glow,
}

/// The `hang` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hang {
    #[serde(deserialize_with = "nonnegative")]
    pub deep: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub bow: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub along: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub graze: f32,
}

/// The `bow` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bow {
    #[serde(deserialize_with = "nonnegative")]
    pub share: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub most: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub close: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub round: f32,
}

/// The `onto` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Onto {
    #[serde(deserialize_with = "nonnegative")]
    pub share: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub most: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lean: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub aside: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub into: f32,
}

/// The `sized` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Sized {
    #[serde(deserialize_with = "positive")]
    pub half: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub most: f32,
}

/// The `wide` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wide {
    #[serde(deserialize_with = "positive")]
    pub aim: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub target: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub source: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub attack: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub block: f32,
}

/// The `arrow` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Arrow {
    #[serde(deserialize_with = "nonnegative")]
    pub tail: f32,
    #[serde(deserialize_with = "positive")]
    pub ramp: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub rise: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub flare: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub flare_at: f32,
    #[serde(deserialize_with = "positive")]
    pub flare_spread: f32,
    #[serde(deserialize_with = "positive")]
    pub fade: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub fade_power: f32,
}

/// The `swell` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Swell {
    #[serde(deserialize_with = "interior")]
    pub peak: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least: f32,
    #[serde(deserialize_with = "positive")]
    pub start: f32,
    #[serde(deserialize_with = "positive")]
    pub end: f32,
}

/// The `beam` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Beam {
    #[serde(deserialize_with = "positive")]
    pub head: f32,
    #[serde(deserialize_with = "positive")]
    pub neck: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub dart: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub from: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub to: f32,
}

/// The `glow` group of `Light`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    #[serde(deserialize_with = "nonnegative")]
    pub reach_share: f32,
    #[serde(deserialize_with = "positive")]
    pub reach_pixels: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub halo_share: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub halo_pixels: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub body: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub halo: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub core: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub core_lean: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub core_reach: f32,
    #[serde(deserialize_with = "positive")]
    pub gain: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub fade_pixels: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub width_weight: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub turn_weight: f32,
    pub gains: GlowGains,
}

/// The `glow.gains` group of `Glow`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GlowGains {
    #[serde(deserialize_with = "nonnegative")]
    pub aim: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub target: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub source: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub attack: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub block: f32,
}

/// Tuning of the marks a permanent wears on its own: `own.json`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Own {
    #[serde(deserialize_with = "nonnegative")]
    pub lift: f32,
    #[serde(deserialize_with = "positive")]
    pub fade_ms: f32,
    pub moon: OwnIcon,
    pub moon_badge: MoonBadge,
    pub coin: OwnIcon,
    pub away: AwayHalo,
    pub mythic_stack: MythicStack,
    pub wedge: Wedge,
    pub crescent: Crescent,
    pub combat_fx: CombatFx,
}

/// Enhanced-tier procedural combat visuals (`own.json`'s `combat_fx`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CombatFx {
    pub attacker: AttackerFx,
    pub blocker: BlockerFx,
}

/// Geometry and ember sparks for the Enhanced attacking chevron.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttackerFx {
    /// Base width as a share of the edge facing the defender.
    #[serde(deserialize_with = "positive")]
    pub width_scale: f32,
    /// Unclamped apex height as a share of the edge facing the defender.
    #[serde(deserialize_with = "positive")]
    pub height_scale: f32,
    /// Minimum and maximum apex height in card widths.
    #[serde(deserialize_with = "vector")]
    pub height_range: Vec2,
    pub sparks: CombatSparks,
}

/// Deterministic looping ember sparks drifting from an Enhanced attacking chevron.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CombatSparks {
    pub unblocked_count: u32,
    pub blocked_count: u32,
    /// `[base, span]` loop period in seconds.
    #[serde(deserialize_with = "vector")]
    pub cycle_s: Vec2,
    /// Lateral start position along the chevron's slanted edge, as a share of base width.
    #[serde(deserialize_with = "positive")]
    pub edge_inset: f32,
    /// `[min, span]` fractional position along the chevron slope from base to apex.
    #[serde(deserialize_with = "vector")]
    pub along: Vec2,
    /// Sinusoidal lateral sway cycles over one spark lifetime.
    #[serde(deserialize_with = "nonnegative")]
    pub sway_turns: f32,
    /// Lateral sway amplitude in card widths.
    #[serde(deserialize_with = "nonnegative")]
    pub sway_reach: f32,
    /// `[min, span]` outward drift distance in card widths over one spark lifetime.
    #[serde(deserialize_with = "vector")]
    pub drift: Vec2,
    /// `[min, span]` initial spark radius in card widths.
    #[serde(deserialize_with = "vector")]
    pub radius: Vec2,
    /// Share of initial radius lost by the end of a spark's life.
    #[serde(deserialize_with = "fraction")]
    pub shrink: f32,
    /// Inverse fade-in fraction of a spark's life.
    #[serde(deserialize_with = "positive")]
    pub fade_in: f32,
}

/// Geometry for the Enhanced blocking shield dome.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockerFx {
    /// Span width as a share of the edge facing the attacker.
    #[serde(deserialize_with = "positive")]
    pub width_scale: f32,
    /// Target dome apex height in card widths.
    #[serde(deserialize_with = "positive")]
    pub height_card: f32,
    /// Upper clamp on dome apex height as a share of the edge width.
    #[serde(deserialize_with = "positive")]
    pub height_edge_max: f32,
}

/// A summoning-sickness moon or token coin: its size and middle, in card widths and heights.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OwnIcon {
    #[serde(deserialize_with = "positive")]
    pub size: f32,
    #[serde(deserialize_with = "vector")]
    pub at: Vec2,
}

/// The halo round a card shown away from the zone it is in, such as a card a hand shows that
/// its player may cast from another zone (`own.json`'s `away`): two soft halos, an outer violet
/// one and an inner pale one, each breathing in and out of step with the other so that the
/// light seems to drift round the card like smoke.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AwayHalo {
    /// The outer halo's size over the size of the halo that fits the card exactly.
    #[serde(deserialize_with = "positive")]
    pub reach: f32,
    /// The inner halo's size, as `reach` is.
    #[serde(deserialize_with = "positive")]
    pub inner: f32,
    /// How much each halo grows and shrinks, as a share of its size.
    #[serde(deserialize_with = "interior")]
    pub breath: f32,
    /// How long one breath takes.
    #[serde(deserialize_with = "positive")]
    pub period_ms: f32,
}

/// The Enhanced-tier aura round a mythic rare spell on the stack, and the burst it leaves as the
/// spell leaves the stack (`own.json`'s `mythic_stack`). MTG Arena outlines such a spell in a
/// looping aura tinted by its colors and bursts it as the spell resolves or is countered
/// (`9-iWvWANENU` at 28:46).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MythicStack {
    /// How far the aura's quad reaches past each side of the card, as a share of its width.
    /// Must match `MYTHIC_STACK_PAD` in `fx/mythic_stack.wgsl`.
    #[serde(deserialize_with = "positive")]
    pub pad: f32,
    /// How far past each side the outro burst's quad reaches, as a share of the card's width.
    #[serde(deserialize_with = "positive")]
    pub outro_pad: f32,
    /// How long the outro burst plays once the spell begins to leave the stack.
    #[serde(deserialize_with = "positive")]
    pub outro_ms: f32,
    /// The Baseline tier's aura: two halos in the spell's colors, breathing as `away`'s do.
    pub halo: AwayHalo,
    /// How far the inner Baseline halo's color lies toward white, from 0 (the spell's color) to
    /// 1 (white).
    #[serde(deserialize_with = "interior")]
    pub halo_core: f32,
}

/// The dark round badge behind the summoning-sickness moon, as shares of its diameter.
/// The web's `.card-sick-moon` is 1.7 em wide, with a 0.1 em border and a 1.15 em icon.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MoonBadge {
    #[serde(deserialize_with = "interior")]
    pub border: f32,
    #[serde(deserialize_with = "interior")]
    pub icon: f32,
}

/// The attacker's chevron.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wedge {
    #[serde(deserialize_with = "positive")]
    pub chevron: f32,
    /// Scale along and away from the card edge, including the halo.
    #[serde(deserialize_with = "vector")]
    pub scale: Vec2,
    #[serde(deserialize_with = "positive")]
    pub pop_ms: f32,
    #[serde(deserialize_with = "vector")]
    pub pop: Vec2,
}

/// The blocker's crescent.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Crescent {
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "vector")]
    pub size: Vec2,
    #[serde(deserialize_with = "positive")]
    pub open_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub opens_from: f32,
}

impl Light {
    /// Parses `light.json`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let light: Self = read("light.json", text)?;
        if light.beam.neck <= light.beam.head {
            return Err("light.json: beam.neck: must be past beam.head".to_owned());
        }
        if light.sized.least > light.sized.most {
            return Err("light.json: sized.most: must be at least sized.least".to_owned());
        }
        Ok(light)
    }
}

impl Own {
    /// Parses `own.json`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let own: Self = read("own.json", text)?;
        if own.wedge.scale.min_element() <= 0.0 || own.wedge.scale.max_element() > 1.0 {
            return Err("own.json: wedge.scale: each component must be in (0, 1]".to_owned());
        }
        let attacker = &own.combat_fx.attacker;
        if attacker.height_range.x <= 0.0 || attacker.height_range.y < attacker.height_range.x {
            return Err(
                "own.json: combat_fx.attacker.height_range: expected positive [min, max] with max >= min"
                    .to_owned(),
            );
        }
        if attacker.sparks.cycle_s.x <= 0.0 || attacker.sparks.cycle_s.y < 0.0 {
            return Err(
                "own.json: combat_fx.attacker.sparks.cycle_s: expected [positive, nonnegative]"
                    .to_owned(),
            );
        }
        if attacker.sparks.radius.x <= 0.0 || attacker.sparks.radius.y < 0.0 {
            return Err(
                "own.json: combat_fx.attacker.sparks.radius: expected [positive, nonnegative]"
                    .to_owned(),
            );
        }
        Ok(own)
    }
}
