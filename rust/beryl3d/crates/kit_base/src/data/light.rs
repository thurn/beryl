//! Tuning of arrows, lines, marks, combat emblems and the shapes of light they draw from.

use glam::Vec2;
use serde::Deserialize;

use super::tuning::{interior, nonnegative, positive, read, vector};

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
    pub coin: OwnIcon,
    pub wedge: Wedge,
    pub crescent: Crescent,
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

/// The attacker's chevron.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wedge {
    #[serde(deserialize_with = "positive")]
    pub chevron: f32,
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
        read("own.json", text)
    }
}
