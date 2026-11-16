//! How far the `shake` effect moves the whole picture (docs/beryl-3d.md, "Effects": "`shake`
//! moves the picture, not the table"): side to side and a little up and down, twice, the second
//! swing smaller, as the web interface's `.stage[data-shaking]` rule in
//! `ui/src/stage/effects.css` swings the stage.

use std::f32::consts::PI;

use glam::Vec2;

use super::{Effect, EffectRun};
use crate::data::space::effects::shake;

/// How far the runs in `runs` shake the picture at `now`, in physical pixels, x right and y
/// down, for a window of `scale` physical pixels per logical point. At a `shake` run's eased
/// progress `p` it is `shake.x` × sin(4πp) × (1 − p) across and `shake.y` × sin(8πp) × (1 − p)
/// down, in logical points: zero at the run's start and end. Two runs at once add up.
#[must_use]
pub fn offset(runs: &[EffectRun], now: u64, scale: f32) -> Vec2 {
    runs.iter()
        .filter(|run| {
            run.effect == Effect::Shake && run.start <= now && now < run.start + run.duration
        })
        .map(|run| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "an effect lasts seconds at most"
            )]
            let p = run
                .curve
                .ease((now - run.start) as f32 / run.duration as f32);
            let left = 1.0 - p;
            Vec2::new(
                shake::X * (4.0 * PI * p).sin() * left,
                shake::Y * (8.0 * PI * p).sin() * left,
            ) * scale
        })
        .sum()
}
