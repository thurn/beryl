//! The desktop host's diagnostic events. Times are nanoseconds on its monotonic clock.

use std::time::Instant;

use beryl_render::DrawFailure;
use kit_ui::input::Tier;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct DisplayTick {
    pub sequence: u64,
    /// Incremented on each resume, so gaps across deliberate pauses can be excluded.
    pub epoch: u64,
    pub requested_hz: f32,
    pub callback_ns: u64,
    /// Core Animation's previous and target display times. These are display-link timing
    /// signals and do not identify when Beryl's drawable was presented.
    pub display_ns: Option<i64>,
    pub target_ns: Option<i64>,
    pub reported_hz: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PacerState {
    pub at_ns: u64,
    pub epoch: u64,
    pub running: bool,
    pub requested_hz: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CpuTiming {
    /// Time handling session input between ticks, including layout it triggers.
    pub session_input_ns: u64,
    /// Drive and feed updates and delivery of finished screenshots.
    pub before_frame_ns: u64,
    pub asset_delivery_ns: u64,
    pub prepare_ns: u64,
    pub client_ns: u64,
    pub draw_ns: u64,
    pub drawable_wait_ns: Option<u64>,
    pub encode_submit_ns: Option<u64>,
    pub present_call_ns: Option<u64>,
    pub after_draw_ns: u64,
    pub feed_after_ns: u64,
    pub requests_ns: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Submitted,
    Idle,
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
}

impl From<DrawFailure> for Outcome {
    fn from(reason: DrawFailure) -> Self {
        match reason {
            DrawFailure::Timeout => Self::Timeout,
            DrawFailure::Occluded => Self::Occluded,
            DrawFailure::Outdated => Self::Outdated,
            DrawFailure::Lost => Self::Lost,
            DrawFailure::Validation => Self::Validation,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsTier {
    Baseline,
    Enhanced,
}

impl From<Tier> for GraphicsTier {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Baseline => Self::Baseline,
            Tier::Enhanced => Self::Enhanced,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct FrameTiming {
    #[serde(flatten)]
    pub tick: DisplayTick,
    pub handled_ns: u64,
    pub finished_ns: u64,
    pub outcome: Outcome,
    /// When CPU submission of the render commands returned, before asking wgpu to present.
    pub submitted_ns: Option<u64>,
    pub cpu: Option<CpuTiming>,
    pub render_size: [u32; 2],
    pub fullscreen: bool,
    pub tier: GraphicsTier,
    pub animating: bool,
    pub searching: bool,
    /// Previously submitted scene work whose completion the GPU worker had yet to observe.
    pub gpu_in_flight: Option<u32>,
    /// wgpu currently leaves the platform drawable and its presentation timestamp private.
    pub presented_ns: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct GpuTiming {
    pub sequence: u64,
    pub submit_ns: u64,
    pub observed_ns: u64,
    pub main_pass_gpu_ns: Option<u64>,
    #[serde(default)]
    pub main_pass_sampled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PacingChange {
    pub at_ns: u64,
    pub previous_hz: f32,
    pub new_hz: f32,
    pub previous_tier: GraphicsTier,
    pub new_tier: GraphicsTier,
    /// When absent, a graphics setting changed the tier.
    pub adaptive: Option<crate::rate::Decision>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    Frame(FrameTiming),
    Pacer(PacerState),
    Change(PacingChange),
    Gpu(GpuTiming),
}

impl Timing {
    #[must_use]
    pub fn at_ns(&self) -> u64 {
        match self {
            Self::Frame(frame) => frame.handled_ns,
            Self::Pacer(state) => state.at_ns,
            Self::Change(change) => change.at_ns,
            Self::Gpu(gpu) => gpu.observed_ns,
        }
    }
}

pub(crate) fn elapsed(epoch: Instant) -> u64 {
    epoch.elapsed().as_nanos() as u64
}
