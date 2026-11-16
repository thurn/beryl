//! How the effects on a card change the way it is drawn (docs/beryl-3d.md, "Effects"): `settle`
//! washes it white under a burst of light, `dissolve` darkens it and breaks it up from the top
//! down, `dim` darkens and fades it, `blanch` washes it white and fades it, and `cover` washes
//! it white while its face changes. Each is a function of the run's eased progress alone, with
//! the shares of the run in `effects.<name>` of `data/beryl3d/space.json`, set from the
//! keyframes of `ui/src/stage/effects.css`.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Vec3};

use super::{Effect, EffectRun};
use crate::data::space::effects::{blanch, cover, dim, dissolve, settle};
use crate::data::space::light;
use crate::frame::{Band, Draw, FrameBuilder, sprites};
use crate::table::{CARD_WIDTH, Look};
use crate::tokens::color;

/// The runs in `runs` that play on `on` at `now`, each with its eased progress from 0 to 1.
fn under_way(runs: &[EffectRun], on: Subject, now: u64) -> impl Iterator<Item = (Effect, f32)> {
    runs.iter()
        .filter(move |run| run.on == on && run.start <= now && now < run.start + run.duration)
        .map(move |run| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "an effect lasts seconds at most"
            )]
            let through = (now - run.start) as f32 / run.duration as f32;
            (run.effect, run.curve.ease(through))
        })
}

/// How far `p` is from `from` to `to`, from 0 to 1.
fn share(p: f32, from: f32, to: f32) -> f32 {
    ((p - from) / (to - from)).clamp(0.0, 1.0)
}

/// How white a `settle` card is at progress `p`: as it is until `settle.still` (two sixtieths
/// of a second of its run), whole white by `settle.white`, and back to its face by the end.
fn settle_white(p: f32) -> f32 {
    if p < settle::WHITE {
        share(p, settle::STILL, settle::WHITE)
    } else {
        1.0 - share(p, settle::WHITE, 1.0)
    }
}

/// Multiplies `tint` by `by`.
fn darken(tint: [u8; 3], by: f32) -> [u8; 3] {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a channel from 0 to 255 times a factor from 0 to 1"
    )]
    tint.map(|channel| (f32::from(channel) * by).round() as u8)
}

/// Changes `look`, how the card `on` is drawn at `now`, for the runs in `runs` that play on it.
/// `earlier_face` is the face of the record the card showed before the playing script, which a
/// `cover` shows until `cover.face` of its run; `look.face` is the new record's.
pub fn look(runs: &[EffectRun], on: Subject, now: u64, earlier_face: Option<u16>, look: &mut Look) {
    for (effect, p) in under_way(runs, on, now) {
        match effect {
            Effect::Settle => look.whiten = look.whiten.max(settle_white(p)),
            Effect::Dissolve => {
                let dark = 1.0 - (1.0 - dissolve::DARK) * share(p, 0.0, dissolve::DARKENED);
                look.tint = darken(look.tint, dark);
                look.dissolve = look.dissolve.max(share(p, dissolve::DARKENED, 1.0));
            }
            Effect::Dim => {
                look.tint = darken(
                    look.tint,
                    1.0 - (1.0 - dim::DARK) * share(p, 0.0, dim::DARKENED),
                );
                look.pose.opacity *= 1.0 - p;
            }
            Effect::Blanch => {
                look.whiten = look.whiten.max(share(p, 0.0, blanch::WHITE));
                look.pose.opacity *= 1.0 - share(p, blanch::WHITE, 1.0);
            }
            Effect::Cover => {
                let white = if p < cover::HELD {
                    share(p, 0.0, cover::WHITE)
                } else {
                    1.0 - share(p, cover::HELD, 1.0)
                };
                look.whiten = look.whiten.max(white);
                if p < cover::FACE {
                    look.face = earlier_face;
                }
            }
            _ => {}
        }
    }
}

/// Writes the burst of light over the card `on`, drawn as `look`, for each `settle` run in `runs`
/// that plays on it at `now`: one glow `settle.burst_cards` cards wide, as bright as the card is
/// white.
pub fn write(
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    look: &Look,
    builder: &mut FrameBuilder<'_>,
) {
    for (effect, p) in under_way(runs, on, now) {
        let white = settle_white(p);
        if effect != Effect::Settle || white <= 0.0 {
            continue;
        }
        let size = settle::BURST_CARDS * CARD_WIDTH * look.pose.scale;
        let transform = Affine3A::from_scale_rotation_translation(
            Vec3::new(size, size, 1.0),
            look.pose.rotation,
            look.pose.position + look.pose.rotation * Vec3::Z * light::LIFT,
        );
        builder.draw(
            Band::Glows,
            Draw::glow(transform, color::SETTLE_BURST, sprites::BURST, 1.0 - white),
        );
    }
}
