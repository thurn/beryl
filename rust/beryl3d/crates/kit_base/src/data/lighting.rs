//! The light the table is lit by and the shadows it casts, from `lighting.json`.
//!
//! `shadow` is shared by both visual tiers: the Baseline tier's contact shadows, which the table
//! draws as one soft quad under each raised card, and the Enhanced tier's shadow map fall along
//! the same light and darken what they cover by the same amount, so a card casts its shadow at
//! the same place in both. `enhanced` is read by the renderer alone.

use glam::{Vec2, Vec3};
use serde::Deserialize;
use serde::de::{Deserializer, Error};

use super::tuning::{finite, fraction, nonnegative, positive, positive_fraction, read, vector};

/// Everything `lighting.json` holds.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    pub shadow: Shadow,
    pub enhanced: Enhanced,
}

impl Lighting {
    /// Parses and validates the text of `lighting.json`. Errors name the file and the key.
    pub fn parse(text: &str) -> Result<Self, String> {
        read("lighting.json", text)
    }
}

/// How shadows fall and how dark they are, in both tiers.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    /// How far a shadow falls across the table for each millimetre of the caster's height: x
    /// to the right and y toward the viewer. The light hangs the opposite way.
    #[serde(deserialize_with = "vector")]
    pub light: Vec2,
    /// How much of what a shadow covers it darkens at its fullest: the opacity of a contact
    /// shadow, and the share of the color a fragment in the shadow map's full shadow loses.
    #[serde(deserialize_with = "fraction")]
    pub darkness: f32,
    pub contact: Contact,
}

impl Shadow {
    /// How far a shadow falls on the table for each millimetre of height, in the table's
    /// space, whose y runs away from the viewer.
    #[must_use]
    pub fn fall(&self) -> Vec2 {
        Vec2::new(self.light.x, -self.light.y)
    }

    /// The unit direction toward the light, in the table's space, where z is height. A point
    /// at height h casts its shadow `fall() * h` across the table, so the light lies the
    /// opposite way, one millimetre up for each `light` across.
    #[must_use]
    pub fn toward_light(&self) -> Vec3 {
        (-self.fall()).extend(1.0).normalize()
    }
}

/// The Baseline tier's contact shadows, in millimetres.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    /// The height at which a shadow reaches its full darkness: below it, it comes in with
    /// height.
    #[serde(deserialize_with = "positive")]
    pub emerging: f32,
    /// How much a shadow grows on each side for each millimetre of height, over which it fades.
    #[serde(deserialize_with = "nonnegative")]
    pub penumbra: f32,
    /// A card no higher than this above what lies under it casts no shadow.
    #[serde(deserialize_with = "nonnegative")]
    pub least_height: f32,
    /// How far above what it falls on a shadow is drawn, so that it is not lost in it.
    #[serde(deserialize_with = "nonnegative")]
    pub above: f32,
}

/// The Enhanced tier's lights, shadow map and bloom.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Enhanced {
    pub light: Lights,
    pub shadow_map: ShadowMap,
    pub bloom: Bloom,
}

/// The strengths of the Enhanced tier's lights. The ambient term is computed from them, so that
/// a card lying flat in the open keeps its Baseline color.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lights {
    /// The key light, which casts the shadow map's shadows from `Shadow::toward_light`.
    #[serde(deserialize_with = "fraction")]
    pub key: f32,
    /// The soft fill from the side of the table opposite the key.
    #[serde(deserialize_with = "fraction")]
    pub fill: f32,
    /// How many degrees above the table the fill shines from.
    #[serde(deserialize_with = "elevation")]
    pub fill_elevation: f32,
    /// The strength of the white highlight where a card mirrors the key toward the camera.
    #[serde(deserialize_with = "nonnegative")]
    pub sheen: f32,
    /// How tight that highlight is: the power of its falloff.
    #[serde(deserialize_with = "positive")]
    pub sheen_power: f32,
}

/// The depth map the Enhanced tier draws the cards into from the key light.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShadowMap {
    /// Its width and height in pixels, at most 8192, the largest texture wgpu's default limits
    /// allow.
    #[serde(deserialize_with = "map_size")]
    pub size: u32,
    /// The depth steps added to every caster's depth, so a card does not shadow itself.
    pub bias: i32,
    /// The bias added for each unit of a caster's depth slope.
    #[serde(deserialize_with = "finite")]
    pub slope_bias: f32,
}

/// The Enhanced tier's bloom on glows and streaks.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bloom {
    /// The size of the targets the glows are blurred in, as a share of the frame's.
    #[serde(deserialize_with = "positive_fraction")]
    pub scale: f32,
    /// How many taps the Gaussian blur takes: an odd number up to `BLOOM_TAPS`.
    #[serde(deserialize_with = "taps")]
    pub taps: usize,
    /// The Gaussian's standard deviation, in texels of the small targets.
    #[serde(deserialize_with = "positive")]
    pub sigma: f32,
    /// How much of the blurred light is added onto the picture.
    #[serde(deserialize_with = "nonnegative")]
    pub strength: f32,
}

impl Bloom {
    /// How many taps lie on each side of the blur's centre.
    #[must_use]
    pub fn radius(&self) -> usize {
        self.taps / 2
    }
}

/// The most taps the bloom shader's uniforms hold weights for: 15 on each side of the centre.
pub const BLOOM_TAPS: usize = 31;

fn elevation<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(de)?;
    if (0.0..=90.0).contains(&value) {
        return Ok(value);
    }
    Err(D::Error::custom(format!(
        "expected degrees from 0 to 90, got {value}"
    )))
}

fn map_size<'de, D: Deserializer<'de>>(de: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(de)?;
    if (1..=8192).contains(&value) {
        return Ok(value);
    }
    Err(D::Error::custom(format!(
        "expected a size from 1 to 8192 pixels, got {value}"
    )))
}

fn taps<'de, D: Deserializer<'de>>(de: D) -> Result<usize, D::Error> {
    let value = usize::deserialize(de)?;
    if value % 2 == 1 && value <= BLOOM_TAPS {
        return Ok(value);
    }
    Err(D::Error::custom(format!(
        "expected an odd count up to {BLOOM_TAPS}, got {value}"
    )))
}
