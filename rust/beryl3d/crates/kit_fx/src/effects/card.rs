//! Samples the effects that change a card or ability's surface. The remaining hand-written
//! surfaces use the keyframe shares in `data/beryl3d/space-constants.json`.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Vec3};

use super::{Effect, EffectRun, player};
use kit_base::data::effect::{Effects, Primitive, Surface};
use kit_base::data::space::card::{HEIGHT as CARD_HEIGHT, WIDTH as CARD_WIDTH};
use kit_base::data::space::effects::{blanch, cover, dim, settle};
use kit_base::data::space::light;
use kit_base::pose::Pose;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, EffectLook, FrameBuilder, sprites};
use kit_ui::input::Tier;

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

impl Look {
    /// A face at its natural color, before effects, marks and pointer feedback.
    #[must_use]
    pub fn new(
        pose: Pose,
        face: Option<u16>,
        full: Option<[u16; 2]>,
        state: u16,
        interactive: bool,
    ) -> Self {
        Self {
            pose,
            tint: [255; 3],
            dissolve: 0.0,
            whiten: 0.0,
            face,
            full,
            state,
            interactive,
        }
    }
}

/// The runs in `runs` that play on `on` at `now`, each with its eased progress from 0 to 1.
fn under_way(runs: &[EffectRun], on: Subject, now: u64) -> impl Iterator<Item = (EffectRun, f32)> {
    runs.iter()
        .filter(move |run| run.on == on)
        .filter_map(move |run| Some((*run, run.curve.ease(run.elapsed(now)?))))
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
pub fn look(
    data: (&Effects, Tier),
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    earlier_face: Option<u16>,
    look: &mut Look,
) {
    look_with_face(data, runs, on, now, || earlier_face, look);
}

/// Like `look`, taking the earlier face only if a cover effect shows it.
pub fn look_with_face(
    (effects, tier): (&Effects, Tier),
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    earlier_face: impl FnOnce() -> Option<u16>,
    look: &mut Look,
) {
    let mut earlier_face = Some(earlier_face);
    for (run, p) in under_way(runs, on, now) {
        if let Some(definition) = effects.get(run.effect.name()) {
            for (primitive, p) in player::layers(definition, run, now, tier) {
                if let Primitive::Surface {
                    surface, amount, ..
                } = primitive
                {
                    let value = amount.at(p).clamp(0.0, 1.0);
                    match *surface {
                        Surface::Whiten => look.whiten = look.whiten.max(value),
                        Surface::Tint {
                            color: [r, g, b, _],
                        } => {
                            look.tint = player::tint(look.tint, [r, g, b], value);
                        }
                        Surface::Dissolve => look.dissolve = look.dissolve.max(value),
                        Surface::Opacity => look.pose.opacity *= value,
                        Surface::TextTint { .. } => {}
                    }
                }
            }
            continue;
        }
        match run.effect {
            Effect::Settle => look.whiten = look.whiten.max(settle_white(p)),
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
    effects: &Effects,
    runs: &[EffectRun],
    on: Subject,
    now: u64,
    look: &Look,
    builder: &mut FrameBuilder<'_>,
) {
    for (run, p) in under_way(runs, on, now) {
        let effect = run.effect;
        if let Some(definition) = effects.get(effect.name()) {
            for (primitive, p) in player::layers(definition, run, now, builder.tier()) {
                if let Primitive::Surface {
                    amount,
                    record: true,
                    ..
                } = primitive
                {
                    record(builder, effect, on, look, amount.at(p).clamp(0.0, 1.0));
                }
            }
            continue;
        }
        if let Some(strength) = strength(effect, p) {
            record(builder, effect, on, look, strength);
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

/// Records the strength of a surface change for the match tools.
fn record(builder: &mut FrameBuilder<'_>, effect: Effect, on: Subject, look: &Look, strength: f32) {
    let face = look.pose.frame(
        Vec3::new(CARD_WIDTH, CARD_HEIGHT, 1.0) * look.pose.scale,
        look.pose.position,
    );
    builder.effect(EffectLook {
        name: effect.name(),
        on: Some(on),
        model: Draw::glow(face, [0; 4], sprites::BURST, 0.0).model,
        opacity: strength * look.pose.opacity,
    });
}
