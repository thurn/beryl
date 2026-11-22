//! The display of notices and cards presented above the table.

use glam::Vec2;
use serde::Deserialize;

use super::tuning::{finite, fraction, nonnegative, positive, vector};

/// The time shares over which a notice fades in and out.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FadeShares {
    #[serde(deserialize_with = "fraction")]
    pub r#in: f32,
    #[serde(deserialize_with = "fraction")]
    pub out: f32,
}

/// Fade timing for each kind of notice.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Notices {
    pub turn: FadeShares,
    pub pill: FadeShares,
    pub banner: FadeShares,
}

/// The revealed cards fan, its desktop position and the pill that opens it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Revealed {
    #[serde(deserialize_with = "vector")]
    pub at: Vec2,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub reach: f32,
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "finite")]
    pub pill_x: f32,
    #[serde(deserialize_with = "finite")]
    pub pill_y: f32,
}

/// The distance in front of the camera of a card shown over the flat layer.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shown {
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    pub opening: Opening,
}

/// Durations for revealing, keeping and rearranging the opening hand.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    #[serde(deserialize_with = "nonnegative")]
    pub flip_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub kept_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub cross_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub shift_ms: f32,
}
