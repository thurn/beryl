//! Shared presentation tuning for the game HUD.

use serde::Deserialize;

use super::tuning::{fraction, nonnegative, positive};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub steps: StepTrack,
    pub incoming: IncomingStyle,
    pub opening: OpeningStyle,
    pub banner_ms: u32,
    #[serde(deserialize_with = "nonnegative")]
    pub banner_gap: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StepTrack {
    #[serde(deserialize_with = "fraction")]
    pub off: f32,
    #[serde(deserialize_with = "fraction")]
    pub fixed: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IncomingStyle {
    #[serde(deserialize_with = "positive")]
    pub line: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lethal_glow_blur: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpeningStyle {
    #[serde(deserialize_with = "nonnegative")]
    pub waiting_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub waiting_height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub choice_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub choice_top: f32,
    #[serde(deserialize_with = "positive")]
    pub dot: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub dot_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub dot_inset: f32,
}

impl Hud {
    /// The time a combat banner stays visible, on the frame clock.
    #[must_use]
    pub const fn banner_ns(self) -> u64 {
        self.banner_ms as u64 * 1_000_000
    }
}
