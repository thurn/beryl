//! Tuning for mouse, touch and scrolling.

use serde::Deserialize;

use super::tuning::{nonnegative, positive, positive_fraction, read};

/// How pointer movement is recognized and how it moves cards.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PointerTuning {
    #[serde(deserialize_with = "positive")]
    pub refusal_ms: f32,
    #[serde(deserialize_with = "positive")]
    pub rise_ms: f32,
    #[serde(deserialize_with = "positive")]
    pub fall_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub raise_mm: f32,
    #[serde(deserialize_with = "positive")]
    pub raise_scale: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub nearer: f32,
    #[serde(deserialize_with = "positive")]
    pub held_height_mm: f32,
    /// Height over the table used for a held hand card. Its camera-facing face needs more
    /// clearance for the shadow to extend past it when the lean settles.
    #[serde(deserialize_with = "positive")]
    pub held_hand_height_mm: f32,
    /// The desktop web stage is at least this wide for each point of height.
    #[serde(deserialize_with = "positive")]
    pub held_hand_stage_min_aspect: f32,
    /// How much a hand card grows from pickup to full lift, as in web `Heft`.
    #[serde(deserialize_with = "positive")]
    pub held_hand_scale: f32,
    #[serde(deserialize_with = "positive")]
    pub held_hand_lift_hz: f32,
    #[serde(deserialize_with = "positive")]
    pub max_gap_ms: f32,
    #[serde(deserialize_with = "positive")]
    pub stiffness_hz: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub damping_ratio: f32,
    #[serde(deserialize_with = "positive")]
    pub lean_degrees: f32,
    #[serde(deserialize_with = "positive")]
    pub drag_threshold_points: f32,
    pub long_press_ms: u64,
    #[serde(deserialize_with = "positive")]
    pub grip_points_per_second_squared: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub drag_per_second: f32,
    #[serde(deserialize_with = "positive")]
    pub smooth_velocity_ms: f32,
    #[serde(deserialize_with = "positive")]
    pub smooth_acceleration_ms: f32,
    #[serde(deserialize_with = "positive")]
    pub lean_steps_per_second: f32,
    #[serde(deserialize_with = "positive")]
    pub scroll_step_points: f32,
    #[serde(deserialize_with = "positive")]
    pub wheel_notch_points: f32,
    pub scroll: ScrollTuning,
    pub touch: TouchTuning,
}

/// Keyboard and pointer scrolling, including post-release momentum.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScrollTuning {
    #[serde(deserialize_with = "positive")]
    pub key_step_points: f32,
    pub coast_ms: u64,
    pub sample_ms: u64,
    pub stale_ms: u64,
    #[serde(deserialize_with = "positive")]
    pub max_points_per_second: f32,
}

/// Touch pickup and tap recognition thresholds.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TouchTuning {
    pub together_ms: u64,
    pub tap_ms: u64,
    pub double_ms: u64,
    #[serde(deserialize_with = "positive")]
    pub slop_points: f32,
    pub pickup_ms: u64,
}

impl PointerTuning {
    /// Parses the host's pointer thresholds and gesture timings.
    pub fn parse(text: &str) -> Result<Self, String> {
        let tuning: Self = read("pointer.json", text)?;
        if tuning.scroll.coast_ms == 0 {
            return Err("pointer.json: scroll.coast_ms must be positive".to_owned());
        }
        if tuning.scroll.sample_ms == 0 {
            return Err("pointer.json: scroll.sample_ms must be positive".to_owned());
        }
        // A delayed frame must not run an unbounded number of spring steps.
        if tuning.max_gap_ms / 1000.0 * tuning.lean_steps_per_second > 4096.0 {
            return Err("pointer.json: lean_steps_per_second and max_gap_ms allow more than 4096 spring steps per frame".to_owned());
        }
        Ok(tuning)
    }
}
