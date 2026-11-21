//! How the effects on a card change the way it is drawn (docs/beryl-3d.md, "Effects"): `settle`
//! washes it white under a burst of light, `dissolve` darkens it and breaks it up from the top
//! down, `dim` darkens and fades it, `blanch` washes it white and fades it, and `cover` washes
//! it white while its face changes. Each is a function of the run's eased progress alone, with
//! the shares of the run in `effects.<name>` of `data/beryl3d/space.json`, set from the
//! keyframes of `ui/src/stage/effects.css`.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Vec3};

use super::{Effect, EffectRun};
use crate::data::space::card::{HEIGHT as CARD_HEIGHT, WIDTH as CARD_WIDTH};
use crate::data::space::effects::{blanch, cover, dim, dissolve, settle};
use crate::data::space::light;
use crate::frame::{Band, Draw, EffectLook, FrameBuilder, sprites};
use crate::pose::Pose;
use crate::tokens::color;

/// How one card is drawn in a frame: made from its pose, face and state, and then changed in
/// turn by the effects on it, the marks on it and the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub pose: Pose,
    /// Multiplies the face's color.
    pub tint: [u8; 3],
    /// How far the card has broken up, from 0 to 1.
    pub dissolve: f32,
    /// How far its face is replaced by white light, from 0 to 1.
    pub whiten: f32,
    /// The board-size layer of its face, if it has one.
    pub face: Option<u16>,
    /// The full-size layers of the same face and of the card back, drawn in place of the
    /// board-size ones where the web interface draws a card's full frame (in a hand, on the
    /// stack), once both have landed (`Faces::full_layers`). A face's board-size picture is the
    /// battlefield frame, without the cost, type line or rules text.
    pub full: Option<[u16; 2]>,
    /// `StateBits`.
    pub state: u16,
    pub interactive: bool,
}

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

/// How white a `cover` card is at progress `p`: whitening until `cover.white`, held until
/// `cover.held`, and back to its face by the end.
fn cover_white(p: f32) -> f32 {
    if p < cover::HELD {
        share(p, 0.0, cover::WHITE)
    } else {
        1.0 - share(p, cover::HELD, 1.0)
    }
}

/// How strongly `effect` shows on a card at progress `p`, as the opacity of the web's overlay
/// of white light or of near black would say: how far the card is washed white, or how much of
/// its color darkening takes away. None for an effect that does not change how a card is drawn.
fn strength(effect: Effect, p: f32) -> Option<f32> {
    match effect {
        Effect::Settle => Some(settle_white(p)),
        Effect::Dissolve => Some((1.0 - dissolve::DARK) * share(p, 0.0, dissolve::DARKENED)),
        Effect::Dim => Some((1.0 - dim::DARK) * share(p, 0.0, dim::DARKENED)),
        Effect::Blanch => Some(share(p, 0.0, blanch::WHITE)),
        Effect::Cover => Some(cover_white(p)),
        _ => None,
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
    look_with_face(runs, on, now, || earlier_face, look);
}

/// Like `look`, taking the earlier face only if a cover effect shows it.
pub(crate) fn look_with_face(
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    earlier_face: impl FnOnce() -> Option<u16>,
    look: &mut Look,
) {
    let mut earlier_face = Some(earlier_face);
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
                look.whiten = look.whiten.max(cover_white(p));
                if p < cover::FACE
                    && let Some(earlier_face) = earlier_face.take()
                {
                    look.face = earlier_face();
                    look.full = None;
                }
            }
            _ => {}
        }
    }
}

/// Writes the burst of light over the card `on`, drawn as `look`, for each `settle` run in `runs`
/// that plays on it at `now`, as the `settle` branch of `flareLight` in `ui/src/stage/flare.ts`
/// draws it: one burst, nothing until `settle.from` of the run, brightest at `settle.peak` and
/// gone by `settle.bloomed`, `settle.burst_halves` of the card's longer half across each way at
/// its widest, growing from 0.55 of that to all of it as it brightens. Records each run that changes how the card is drawn as an `EffectLook` over its face,
/// at the opacity `strength` gives times the card's own.
pub fn write(
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    look: &Look,
    builder: &mut FrameBuilder<'_>,
) {
    for (effect, p) in under_way(runs, on, now) {
        if let Some(strength) = strength(effect, p) {
            let face = look.pose.frame(
                Vec3::new(CARD_WIDTH, CARD_HEIGHT, 1.0) * look.pose.scale,
                look.pose.position,
            );
            builder.effect(EffectLook {
                effect,
                on,
                // A record's model is its transform as `Draw` lays it out.
                model: Draw::glow(face, [0; 4], sprites::BURST, 0.0).model,
                opacity: strength * look.pose.opacity,
            });
        }
        if effect != Effect::Settle {
            continue;
        }
        let risen = share(p, settle::FROM, settle::PEAK);
        let bright = if p < settle::PEAK {
            risen
        } else {
            1.0 - share(p, settle::PEAK, settle::BLOOMED)
        };
        if bright <= 0.0 {
            continue;
        }
        let size = 2.0
            * settle::BURST_HALVES
            * (CARD_HEIGHT * 0.5)
            * look.pose.scale
            * (0.55 + 0.45 * risen);
        let transform = Affine3A::from_scale_rotation_translation(
            Vec3::new(size, size, 1.0),
            look.pose.rotation,
            look.pose.position + look.pose.rotation * Vec3::Z * light::LIFT,
        );
        builder.draw(
            Band::Glows,
            Draw::glow(transform, color::SETTLE_BURST, sprites::BURST, 1.0 - bright),
        );
    }
}
