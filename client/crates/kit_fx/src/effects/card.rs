//! Samples the effect layers drawn with a surface and its resolved pose.

use std::sync::Arc;

use super::{EffectRun, Reach, player};
use kit_base::data::effect::{Effects, Primitive, Surface};
use kit_base::pose::Pose;
use kit_frame::frame::{Draw, EffectTarget, FrameBuilder, sprites};
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
    /// face used on the table, without the cost, type line or rules text.
    pub full: Option<[u16; 2]>,
    /// `StateBits`.
    pub state: u32,
    pub interactive: bool,
}

impl Look {
    /// A face at its natural color, before effects, marks and pointer feedback.
    #[must_use]
    pub fn new(
        pose: Pose,
        face: Option<u16>,
        full: Option<[u16; 2]>,
        state: u32,
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

/// The timed layers whose run plays on this surface.
fn layers(
    runs: &[EffectRun],
    on: EffectTarget,
    now: u64,
    tier: Tier,
) -> impl Iterator<Item = (&EffectRun, &Primitive, f32)> {
    runs.iter()
        .filter(move |run| run.on == on)
        .flat_map(move |run| {
            player::layers(run, now, tier)
                .map(move |(primitive, progress)| (run, primitive, progress))
        })
}

/// Changes `look`, how the card `on` is drawn at `now`, for the runs in `runs` that play on it.
/// `earlier_face` is the face the card showed before the script; `look.face` is the new face.
pub fn look(
    tier: Tier,
    runs: &[EffectRun],
    on: EffectTarget,
    now: u64,
    earlier_face: Option<u16>,
    look: &mut Look,
) {
    look_with_face(tier, runs, on, now, || earlier_face, look);
}

/// Like `look`, taking the earlier face only if a layer shows it.
pub fn look_with_face(
    tier: Tier,
    runs: &[EffectRun],
    on: EffectTarget,
    now: u64,
    earlier_face: impl FnOnce() -> Option<u16>,
    look: &mut Look,
) {
    let mut earlier_face = Some(earlier_face);
    for (_, primitive, progress) in layers(runs, on, now, tier) {
        let Primitive::Surface {
            surface, amount, ..
        } = primitive
        else {
            continue;
        };
        let value = amount.at(progress).clamp(0.0, 1.0);
        match *surface {
            Surface::Whiten {} => look.whiten = look.whiten.max(value),
            Surface::Tint {
                color: [r, g, b, _],
            } => {
                look.tint = player::tint(look.tint, [r, g, b], value);
            }
            Surface::Dissolve {} => look.dissolve = look.dissolve.max(value),
            Surface::Opacity {} => look.pose.opacity *= value,
            Surface::PreviousFace {} if value > 0.0 => {
                if let Some(earlier_face) = earlier_face.take() {
                    look.face = earlier_face();
                    look.full = None;
                }
            }
            Surface::PreviousFace {} | Surface::TextTint { .. } => {}
        }
    }
}

/// Writes comparison records for surface changes and draws light attached to the surface.
pub fn write(
    effects: &Effects,
    runs: &[EffectRun],
    on: EffectTarget,
    now: u64,
    look: &Look,
    builder: &mut FrameBuilder<'_>,
) {
    let tier = builder.tier();
    for (run, primitive, progress, tier_only) in
        runs.iter().filter(|run| run.on == on).flat_map(|run| {
            player::sampled_layers(run, now, tier).map(move |(layer, progress)| {
                (
                    run,
                    &layer.primitive,
                    progress,
                    layer.baseline_only || layer.enhanced_only,
                )
            })
        })
    {
        match primitive {
            Primitive::Surface {
                amount,
                record: true,
                ..
            } => {
                record(
                    builder,
                    &run.definition.name,
                    on,
                    look,
                    amount.at(progress).clamp(0.0, 1.0),
                );
            }
            Primitive::Burst {
                quad,
                on_surface: true,
            } => {
                let reach = Reach {
                    pose: look.pose,
                    half: builder.card() * (0.5 * look.pose.scale),
                    colors: kit_base::data::effect::SubjectColors::NONE,
                    settle: kit_base::data::effect::SettleKind::Small,
                    mythic: false,
                };
                player::write_surface_burst(
                    quad,
                    (
                        progress,
                        effects.lift() * look.pose.scale,
                        effects.palette(),
                    ),
                    tier_only,
                    run,
                    reach,
                    builder,
                );
            }
            _ => {}
        }
    }
}

/// Records the strength of a surface change for the match tools.
fn record(
    builder: &mut FrameBuilder<'_>,
    name: &Arc<str>,
    on: EffectTarget,
    look: &Look,
    strength: f32,
) {
    let face = look.pose.frame(
        builder.card().extend(1.0) * look.pose.scale,
        look.pose.position,
    );
    builder.effect(
        name,
        Some(on),
        Draw::glow(face, [0; 4], sprites::BURST, 0.0).model,
        strength * look.pose.opacity,
    );
}
