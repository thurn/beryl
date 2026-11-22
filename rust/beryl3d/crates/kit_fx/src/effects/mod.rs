//! Plays timed effect layers on table subjects, surfaces and prepared text.
//! The host supplies the definitions. Script playback supplies the anchor and clock.

pub mod card;
pub mod light;
pub mod particles;
mod player;
pub mod preview;
pub mod shake;

pub use player::{NumberLook, TextLook, text};

use std::sync::Arc;

use beryl_protocol::{protocol::ObjectId, script::Subject};
use glam::{Vec2, Vec3};
use kit_frame::frame::FrameBuilder;

use kit_base::data::effect::{Definition, Effects};
use kit_base::pose::{Curve, Pose};

/// Where something is and how large, for the light drawn to or on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub pose: Pose,
    /// Half its width and height, in millimetres. A player's anchor is a point.
    pub half: Vec2,
}

/// Writes the effects that draw in table space, using the subject's current or earlier pose.
pub fn write(
    effects: &Effects,
    runs: &[EffectRun],
    now: u64,
    reach: impl Fn(Subject, u64) -> Option<Reach>,
    (eye, scale): (Vec3, f32),
    builder: &mut FrameBuilder<'_>,
) {
    light::write(effects, runs, now, &reach, eye, builder);
    particles::write(effects, runs, now, |subject| reach(subject, 0), builder);
    shake::write(runs, now, scale, builder);
}

/// One effect played on `on`, from `start` for `duration` along `curve`, in nanoseconds since
/// the client started once the show holds it. `step` is the number of the step that wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectRun {
    pub definition: Arc<Definition>,
    pub on: Subject,
    pub start: u64,
    pub duration: u64,
    pub curve: Curve,
    pub step: usize,
}

impl EffectRun {
    /// The fraction of the run elapsed at this time, from its start until its end.
    #[must_use]
    pub fn elapsed(&self, now: u64) -> Option<f32> {
        let past = now.checked_sub(self.start)?;
        if past >= self.duration {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a subsecond clock normalized to 0–1"
        )]
        let fraction = past as f32 / self.duration as f32;
        Some(fraction)
    }

    /// Whether the anchor represents a point, such as a life total.
    fn on_point(&self) -> bool {
        matches!(self.on, Subject::Player { .. })
    }

    /// Runs one effect on the preview's single surface, using the same drawing code as play.
    fn preview(definition: Arc<Definition>, duration: u64) -> Self {
        Self {
            definition,
            on: Subject::Object { slot: ObjectId(0) },
            start: 0,
            duration,
            curve: Curve::LINEAR,
            step: 0,
        }
    }
}
