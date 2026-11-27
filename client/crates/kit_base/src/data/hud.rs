//! Shared presentation tuning for the game HUD.

use serde::Deserialize;

use super::tuning::{fraction, nonnegative, positive};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub steps: StepTrack,
    pub incoming: IncomingStyle,
    pub opening: OpeningStyle,
    pub thinking: ThinkingStyle,
    pub banner_ms: u32,
    #[serde(deserialize_with = "nonnegative")]
    pub banner_gap: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThinkingStyle {
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    /// How long the spinner takes to turn once.
    #[serde(deserialize_with = "positive")]
    pub turn_ms: f32,
}

impl ThinkingStyle {
    /// One full turn of the thinking spinner, in nanoseconds.
    #[must_use]
    pub const fn turn_ns(self) -> f32 {
        self.turn_ms * 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StepTrack {
    #[serde(deserialize_with = "fraction")]
    pub off: f32,
    #[serde(deserialize_with = "fraction")]
    pub fixed: f32,
    /// How long a phase icon takes to brighten or dim on hover.
    #[serde(deserialize_with = "nonnegative")]
    pub hover_ms: f32,
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
