//! Plays timed effect layers on table subjects, surfaces and prepared text.
//! The host supplies the definitions. Script playback supplies the anchor and clock.

pub mod card;
pub mod light;
pub mod particles;
mod player;
pub mod preview;
pub mod shake;

pub use player::{NumberLook, TextLook, text};

use beryl_protocol::{protocol::ObjectId, script::Subject};
use glam::{Vec2, Vec3};
use kit_frame::frame::FrameBuilder;

use kit_base::data::effect::{Effects, Primitive, Surface};
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
    shake::write(effects, runs, now, scale, builder);
}

/// One effect, by its name in `data/ui-conventions.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Glow,
    Settle,
    Dissolve,
    Dim,
    Blanch,
    Cover,
    Impact,
    Shake,
    Trail,
    Flash,
    Beam,
    Pulse,
    Loss,
    Gain,
}

impl Effect {
    /// Every effect, in the order of the data file's list.
    pub const ALL: [Self; 14] = [
        Self::Glow,
        Self::Settle,
        Self::Dissolve,
        Self::Dim,
        Self::Blanch,
        Self::Cover,
        Self::Impact,
        Self::Shake,
        Self::Trail,
        Self::Flash,
        Self::Beam,
        Self::Pulse,
        Self::Loss,
        Self::Gain,
    ];

    /// Its name in `data/ui-conventions.json`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Glow => "glow",
            Self::Settle => "settle",
            Self::Dissolve => "dissolve",
            Self::Dim => "dim",
            Self::Blanch => "blanch",
            Self::Cover => "cover",
            Self::Impact => "impact",
            Self::Shake => "shake",
            Self::Trail => "trail",
            Self::Flash => "flash",
            Self::Beam => "beam",
            Self::Pulse => "pulse",
            Self::Loss => "loss",
            Self::Gain => "gain",
        }
    }

    /// Whether this has layers drawn with a surface, using its resolved pose.
    pub fn on_card(self, effects: &Effects) -> bool {
        effects.get(self.name()).is_some_and(|definition| {
            definition.layers.iter().any(|layer| match layer.primitive {
                Primitive::Surface { surface, .. } => !matches!(surface, Surface::TextTint { .. }),
                Primitive::Burst { on_surface, .. } => on_surface,
                _ => false,
            })
        })
    }

    /// The effect named `name` in `data/ui-conventions.json`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effect| effect.name() == name)
    }
}

/// One effect played on `on`, from `start` for `duration` along `curve`, in nanoseconds since
/// the client started once the show holds it. `step` is the number of the step that wrote it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectRun {
    pub effect: Effect,
    pub on: Subject,
    pub start: u64,
    pub duration: u64,
    pub curve: Curve,
    pub step: usize,
}

impl EffectRun {
    /// The fraction of the run elapsed at this time, from its start until its end.
    #[must_use]
    pub fn elapsed(self, now: u64) -> Option<f32> {
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
    fn on_point(self) -> bool {
        matches!(self.on, Subject::Player { .. })
    }

    /// Runs one effect on the preview's single surface, using the same drawing code as play.
    fn preview(effect: Effect, duration: u64) -> Self {
        Self {
            effect,
            on: Subject::Object { slot: ObjectId(0) },
            start: 0,
            duration,
            curve: Curve::LINEAR,
            step: 0,
        }
    }
}
