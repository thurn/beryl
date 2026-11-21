//! Samples effect layers on the frame's clock and sends each primitive to its drawing surface.

use glam::{Vec2, Vec3};
use kit_base::data::effect::{
    Clock, Definition, Effects, NumberOrigin, Primitive, Quad, Shape, Surface,
};
use kit_frame::frame::{Band, FrameBuilder, Sprite, sprites};
use kit_ui::{input::Tier, layout::FloatLook};

use super::{EffectRun, Reach, light};

/// Samples each visible layer at its local progress. An effect's run is half-open in time.
pub(super) fn layers(
    definition: &Definition,
    run: EffectRun,
    now: u64,
    tier: Tier,
) -> impl Iterator<Item = (&Primitive, f32)> {
    let elapsed = run.elapsed(now);
    let eased = elapsed.map(|elapsed| run.curve.ease(elapsed));
    definition.layers.iter().filter_map(move |layer| {
        if layer.enhanced_only && tier != Tier::Enhanced {
            return None;
        }
        let progress = match layer.clock {
            Clock::Eased => eased?,
            Clock::Elapsed => elapsed?,
        };
        (layer.start <= progress && progress <= layer.end).then(|| {
            let local = (progress - layer.start) / (layer.end - layer.start);
            (&layer.primitive, local)
        })
    })
}

fn sprite(shape: Shape) -> Sprite {
    match shape {
        Shape::Disc => sprites::DISC,
        Shape::Cone => sprites::CONE,
        Shape::Comet => sprites::COMET,
        Shape::Ribbon => sprites::RIBBON,
        Shape::Outline => sprites::OUTLINE,
        Shape::Head => sprites::HEAD,
        Shape::Wedge => sprites::WEDGE,
        Shape::Crescent => sprites::CRESCENT,
        Shape::Tap => sprites::TAP,
        Shape::Moon => sprites::MOON,
        Shape::Plaque => sprites::PLAQUE,
        Shape::Coin => sprites::COIN,
        Shape::Halo => sprites::HALO,
        Shape::WedgeHot => sprites::WEDGE_HOT,
    }
}

/// Draws the layers that cast light where the subject is, or along its recent path.
pub(super) fn write_light(
    definition: &Definition,
    run: EffectRun,
    now: u64,
    reach_at: impl Fn(u64) -> Option<Reach>,
    eye: Vec3,
    builder: &mut FrameBuilder<'_>,
) {
    for (primitive, p) in layers(definition, run, now, builder.tier()) {
        match primitive {
            Primitive::Sprite { .. } | Primitive::Ring { .. } | Primitive::Burst { .. } => {
                let Some(reach) = reach_at(0) else { return };
                let basic = reach.half * 2.0;
                let (quad, cell, size) = match primitive {
                    Primitive::Sprite { shape, quad } => {
                        let size = if *shape == Shape::Halo {
                            sprites::halo_size(reach.half)
                        } else {
                            basic
                        };
                        (quad, sprite(*shape), size)
                    }
                    Primitive::Ring { quad } => (quad, sprites::RING, basic),
                    Primitive::Burst { quad } => {
                        (quad, sprites::BURST, Vec2::splat(basic.max_element()))
                    }
                    _ => unreachable!("only the sprite, ring and burst emit quads"),
                };
                write_quad(quad, cell, size, p, run, reach, builder);
            }
            Primitive::Streak {
                lookback_ms,
                width,
                minimum,
                tint,
                opacity,
            } => {
                let before = u64::from(*lookback_ms) * 1_000_000;
                if let (Some(reach), Some(was)) = (reach_at(0), reach_at(before))
                    && let Some(draw) = light::path_streak(
                        reach,
                        was,
                        eye,
                        minimum.at(p),
                        width.at(p),
                        light::fade(*tint, opacity.at(p)),
                    )
                {
                    builder.draw(Band::Glows, draw);
                }
            }
            _ => {}
        }
    }
}

fn write_quad(
    quad: &Quad,
    sprite: Sprite,
    size: Vec2,
    progress: f32,
    run: EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    let fade = quad.fade.at(progress).clamp(0.0, 1.0);
    if let Some(record) = &quad.record {
        light::record_as(
            builder,
            &run,
            &reach.pose,
            reach.half,
            (
                record.scale.at(progress),
                record.opacity.at(progress).clamp(0.0, 1.0),
            ),
        );
    }
    builder.draw(
        Band::Glows,
        light::lying(
            &reach.pose,
            size * quad.scale.at(progress),
            quad.tint,
            sprite,
            fade,
        ),
    );
}

/// The sampled position and opacity of a number already laid out by its host.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberLook {
    pub origin: NumberOrigin,
    pub drift: f32,
    pub opacity: f32,
}

impl NumberLook {
    /// Positions a prepared number beside its subject using the host's layout.
    #[must_use]
    pub fn place(self, point: Vec2, anchor: Vec2, tint: Option<[u8; 4]>) -> FloatLook {
        FloatLook {
            point,
            anchor,
            tint,
            opacity: self.opacity,
            scale: 1.0,
        }
    }
}

/// A text surface and the number floating away from it, if the current layers draw either.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextLook {
    pub tint: Option<[u8; 4]>,
    pub number: Option<NumberLook>,
}

/// Samples the text layers. The game supplies the base color and lays out the actual number
/// before the frame, so starting a float needs neither formatting nor text shaping.
#[must_use]
pub fn text(
    effects: &Effects,
    run: EffectRun,
    now: u64,
    tier: Tier,
    base: [u8; 4],
) -> Option<TextLook> {
    let definition = effects.get(run.effect.name())?;
    let mut look = TextLook::default();
    for (primitive, p) in layers(definition, run, now, tier) {
        match primitive {
            Primitive::Surface {
                surface: Surface::TextTint { color },
                amount,
                ..
            } => {
                look.tint = Some(tint(look.tint.unwrap_or(base), *color, amount.at(p)));
            }
            Primitive::Number {
                origin,
                drift,
                opacity,
            } => {
                look.number = Some(NumberLook {
                    origin: *origin,
                    drift: drift.at(p),
                    opacity: opacity.at(p).clamp(0.0, 1.0),
                });
            }
            _ => {}
        }
    }
    (look.tint.is_some() || look.number.is_some()).then_some(look)
}

/// Composes a tint over an existing channel, preserving exact endpoints.
pub(super) fn tint<const N: usize>(base: [u8; N], color: [u8; N], amount: f32) -> [u8; N] {
    let amount = amount.clamp(0.0, 1.0);
    std::array::from_fn(|index| {
        let value = f32::from(base[index]) * (1.0 - amount) + f32::from(color[index]) * amount;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a rounded interpolation between two bytes"
        )]
        let value = value.round() as u8;
        value
    })
}
