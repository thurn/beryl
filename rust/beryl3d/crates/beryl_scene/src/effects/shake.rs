//! How far the `shake` effect moves the whole picture. Not built yet: it never moves it.

use glam::Vec2;

use super::EffectRun;

/// How far the runs in `runs` shake the picture at `now`, in physical pixels.
#[must_use]
pub fn offset(_runs: &[EffectRun], _now: u64) -> Vec2 {
    Vec2::ZERO
}
