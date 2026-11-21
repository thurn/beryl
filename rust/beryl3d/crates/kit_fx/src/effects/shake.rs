//! How far the `shake` effect moves the whole picture (docs/beryl-3d.md, "Effects": "`shake`
//! moves the picture, not the table"): side to side and a little up and down, twice, the second
//! swing smaller, as the web interface's `.stage[data-shaking]` rule in
//! `ui/src/stage/effects.css` swings the stage.

use std::f32::consts::PI;

use glam::Vec2;

use super::{Effect, EffectRun, player};
use kit_base::data::effect::{Effects, Primitive};
use kit_base::data::space::effects::shake;
use kit_frame::frame::{EffectLook, FrameBuilder};
use kit_ui::input::Tier;

/// Shakes the frame's picture by `offset` of the runs in `runs` at `now`, for a window of `scale`
/// physical pixels per logical point. While a `shake` run is under way it also records an
/// `EffectLook` for the match tools, whose `model` is only a translation by the offset in logical
/// points: a shake moves the picture of the table, not a thing on it, so `tree --records`
/// reports it as a box of no size at that offset.
pub fn write(
    effects: &Effects,
    runs: &[EffectRun],
    now: u64,
    scale: f32,
    builder: &mut FrameBuilder<'_>,
) {
    let (moved, first) = displacement(effects, runs, now, scale, builder.tier());
    builder.shake(moved);
    let points = moved / scale;
    // Two runs at once move the picture once, by their sum, so one record stands for both.
    if let Some(effect) = first {
        builder.effect(EffectLook {
            name: effect.name(),
            on: None,
            model: [
                1.0, 0.0, 0.0, points.x, 0.0, 1.0, 0.0, points.y, 0.0, 0.0, 1.0, 0.0,
            ],
            opacity: 1.0,
        });
    }
}

/// The shake in physical pixels, x right and y down, and the first effect contributing to it.
fn displacement(
    effects: &Effects,
    runs: &[EffectRun],
    now: u64,
    scale: f32,
    tier: Tier,
) -> (Vec2, Option<Effect>) {
    let mut moved = Vec2::ZERO;
    let mut first = None;
    for &run in runs {
        if let Some(definition) = effects.get(run.effect.name()) {
            for (primitive, p) in player::layers(definition, run, now, tier) {
                if let Primitive::Shake { x, y } = primitive {
                    moved += Vec2::new(x.at(p), y.at(p)) * scale;
                    first.get_or_insert(run.effect);
                }
            }
        } else if run.effect == Effect::Shake
            && let Some(elapsed) = run.elapsed(now)
        {
            moved += legacy_offset(run.curve.ease(elapsed)) * scale;
            first.get_or_insert(run.effect);
        }
    }
    (moved, first)
}

/// The shake's offset in logical points. Two swings diminish to zero at the end of the run.
fn legacy_offset(p: f32) -> Vec2 {
    let left = 1.0 - p;
    Vec2::new(
        shake::X * (4.0 * PI * p).sin() * left,
        shake::Y * (8.0 * PI * p).sin() * left,
    )
}

/// How far the runs shake the picture at `now`, in physical pixels.
#[must_use]
pub fn offset(effects: &Effects, runs: &[EffectRun], now: u64, scale: f32, tier: Tier) -> Vec2 {
    displacement(effects, runs, now, scale, tier).0
}
