//! Combines all shake layers into one picture displacement.

use std::sync::Arc;

use glam::Vec2;

use super::{EffectRun, player};
use kit_base::data::effect::Primitive;
use kit_frame::frame::FrameBuilder;
use kit_ui::input::Tier;

/// Shakes the frame's picture by `offset` of the runs in `runs` at `now`, for a window of `scale`
/// physical pixels per logical point. While a `shake` run is under way it also records an
/// `EffectLook` for the match tools, whose `model` is only a translation by the offset in logical
/// points: a shake moves the picture of the table, not a thing on it, so `tree --records`
/// reports it as a box of no size at that offset.
pub fn write(runs: &[EffectRun], now: u64, scale: f32, builder: &mut FrameBuilder<'_>) {
    let (moved, first) = displacement(runs, now, scale, builder.tier());
    builder.shake(moved);
    let points = moved / scale;
    // Two runs at once move the picture once, by their sum, so one record stands for both.
    if let Some(effect) = first {
        builder.effect(
            effect,
            None,
            [
                1.0, 0.0, 0.0, points.x, 0.0, 1.0, 0.0, points.y, 0.0, 0.0, 1.0, 0.0,
            ],
            1.0,
        );
    }
}

/// The shake in physical pixels, x right and y down, and the first effect contributing to it.
fn displacement(runs: &[EffectRun], now: u64, scale: f32, tier: Tier) -> (Vec2, Option<&Arc<str>>) {
    let mut moved = Vec2::ZERO;
    let mut first = None;
    for run in runs {
        for (primitive, p) in player::layers(run, now, tier) {
            if let Primitive::Shake { x, y } = primitive {
                moved += Vec2::new(x.at(p), y.at(p)) * scale;
                first.get_or_insert(&run.definition.name);
            }
        }
    }
    (moved, first)
}

/// How far the runs shake the picture at `now`, in physical pixels.
#[must_use]
pub fn offset(runs: &[EffectRun], now: u64, scale: f32, tier: Tier) -> Vec2 {
    displacement(runs, now, scale, tier).0
}
