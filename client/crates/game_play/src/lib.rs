//! Scripts compiled to tracks; verbs, beats, notices, sound cues, arrows and marks.

pub use game_art::art;
pub mod beat;
pub mod hud;
pub mod light;
pub mod notice;
pub mod play;
pub mod pointer;
pub mod sound;
pub mod verbs;

use beryl_protocol::script::Curve as Named;
use kit_base::pose::Curve;

/// The CSS curve a script names, as `CURVES` in `ui/src/stage/timing.ts` maps it.
#[must_use]
pub const fn named_curve(curve: Named) -> Curve {
    match curve {
        Named::Linear => Curve::LINEAR,
        Named::EaseIn => Curve::EASE_IN,
        Named::EaseOut => Curve::EASE_OUT,
        Named::EaseInOut => Curve::EASE_IN_OUT,
    }
}
