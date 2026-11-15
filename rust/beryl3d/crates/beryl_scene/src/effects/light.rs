//! The effects that are light of their own (a glow, a beam, a flash), drawn over the table. Not
//! built yet: it writes nothing.

use super::EffectRun;
use crate::frame::FrameBuilder;
use crate::play::Scene;

/// Writes the records of the runs in `runs` that are light, as they are at `now`.
pub fn write(_runs: &[EffectRun], _now: u64, _scene: &Scene<'_>, _builder: &mut FrameBuilder<'_>) {}
