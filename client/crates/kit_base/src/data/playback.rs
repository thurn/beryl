//! Timings for playback, scrolling and host frame pacing.

use serde::Deserialize;

use super::tuning::{fraction, nonnegative, nonzero_count, positive, read};

/// How many queued updates start playback at twice or four times the usual speed.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatchUp {
    pub double: usize,
    pub quadruple: usize,
}

impl CatchUp {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let catch_up: Self = read("catch-up.json", text)?;
        if catch_up.quadruple < catch_up.double {
            return Err("catch-up.json: quadruple must be at least double".to_owned());
        }
        Ok(catch_up)
    }
}

/// The length of the slide that follows a scroll of the table.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Slide {
    #[serde(deserialize_with = "nonnegative")]
    pub regroup_ms: f32,
}

/// The host's thresholds for stepping the display rate down and back up.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FrameRate {
    pub settle_frames: usize,
    #[serde(deserialize_with = "positive")]
    pub late_intervals: f32,
    #[serde(deserialize_with = "nonzero_count")]
    pub window: usize,
    #[serde(deserialize_with = "nonzero_count")]
    pub misses: usize,
    #[serde(deserialize_with = "fraction")]
    pub room: f32,
    #[serde(deserialize_with = "nonzero_count")]
    pub up_frames: usize,
    pub up_misses: usize,
    pub up_frames_most: usize,
    #[serde(deserialize_with = "nonzero_count")]
    pub lowest_hz: usize,
}

impl FrameRate {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let rate: Self = read("frame-rate.json", text)?;
        if rate.misses > rate.window {
            return Err("frame-rate.json: misses exceeds the frame window".to_owned());
        }
        if rate.up_misses >= rate.up_frames {
            return Err("frame-rate.json: up_misses is not below up_frames".to_owned());
        }
        if rate.up_frames_most < rate.up_frames {
            return Err("frame-rate.json: up_frames_most is below up_frames".to_owned());
        }
        Ok(rate)
    }
}

/// The startup measurement that decides whether the host can draw the Enhanced tier.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TierProbe {
    #[serde(deserialize_with = "nonzero_count")]
    pub frames: usize,
    #[serde(deserialize_with = "fraction")]
    pub share: f32,
}
