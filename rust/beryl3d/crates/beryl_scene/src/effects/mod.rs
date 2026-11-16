//! Effects: the 14 named looks of `effects` in `data/ui-conventions.json` that a step plays on a
//! card, a player or the whole table (docs/beryl-3d.md, "Effects"). A verb writes each as an
//! `EffectRun` when its script is compiled; `card` changes how a card is drawn for the runs on
//! it, `light` draws the runs that are light of their own, `particles` draws the Enhanced tier's
//! ash and vortex for `vanish`, and `shake` moves the whole picture.

pub mod card;
pub mod light;
pub mod particles;
pub mod shake;

use beryl_protocol::script::Subject;

use crate::pose::Curve;

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

    /// Whether this changes the face or material of a card, or draws its settle burst.
    pub(crate) fn on_card(self) -> bool {
        matches!(
            self,
            Self::Settle | Self::Dissolve | Self::Dim | Self::Blanch | Self::Cover
        )
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
