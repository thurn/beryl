//! Samples effect layers on the frame's clock and sends each primitive to its drawing surface.

use glam::{Quat, Vec2, Vec3};
use kit_base::data::effect::{
    Clock, DirectedSprite, Direction, EffectPalette, Layer, LightRecord, NumberOrigin, Primitive,
    Quad, Scalar, Shape, Surface, Tint, Vfx,
};
use kit_frame::frame::{Band, Draw, FrameBuilder, Sprite, sprites};
use kit_ui::{input::Tier, layout::FloatLook};

use super::{EffectRun, Reach, light};

/// Samples each visible layer at its local progress. An effect's run is half-open in time.
pub(super) fn layers(
    run: &EffectRun,
    now: u64,
    tier: Tier,
) -> impl Iterator<Item = (&Primitive, f32)> {
    sampled_layers(run, now, tier).map(|(layer, local)| (&layer.primitive, local))
}

pub(super) fn sampled_layers(
    run: &EffectRun,
    now: u64,
    tier: Tier,
) -> impl Iterator<Item = (&Layer, f32)> {
    let elapsed = run.elapsed(now);
    let eased = elapsed.map(|elapsed| run.curve.ease(elapsed));
    run.definition.layers.iter().filter_map(move |layer| {
        if layer.enhanced_only && tier != Tier::Enhanced {
            return None;
        }
        if layer.baseline_only && tier == Tier::Enhanced {
            return None;
        }
        let progress = match layer.clock {
            Clock::Eased => eased?,
            Clock::Elapsed => elapsed?,
        };
        (layer.start <= progress && progress <= layer.end).then(|| {
            let local = (progress - layer.start) / (layer.end - layer.start);
            (layer, local)
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
    run: &EffectRun,
    now: u64,
    reach_at: impl Fn(u64) -> Option<Reach>,
    (eye, lift, palette): (Vec3, f32, &EffectPalette),
    builder: &mut FrameBuilder<'_>,
) {
    for (layer, p) in sampled_layers(run, now, builder.tier()) {
        let primitive = &layer.primitive;
        if !matches!(
            primitive,
            Primitive::Sprite { .. }
                | Primitive::Ring { .. }
                | Primitive::Burst {
                    on_surface: false,
                    ..
                }
                | Primitive::Directed { .. }
                | Primitive::Streak { .. }
                | Primitive::Vfx { .. }
        ) {
            continue;
        }
        let Some(reach) = reach_at(0) else { return };
        if layer.mythic_only && !reach.mythic {
            continue;
        }
        let tier_only = layer.baseline_only || layer.enhanced_only;
        write_light_primitive(
            (primitive, p, tier_only),
            reach,
            &reach_at,
            (eye, lift, palette),
            run,
            builder,
        );
    }
}

fn write_light_primitive(
    (primitive, p, tier_only): (&Primitive, f32, bool),
    reach: Reach,
    reach_at: &impl Fn(u64) -> Option<Reach>,
    (eye, lift, palette): (Vec3, f32, &EffectPalette),
    run: &EffectRun,
    builder: &mut FrameBuilder<'_>,
) {
    let basic = reach.half * 2.0;
    match primitive {
        Primitive::Sprite { shape, quad } => {
            let size = if *shape == Shape::Halo {
                sprites::halo_size(reach.half)
            } else {
                basic
            };
            write_quad(
                quad,
                (sprite(*shape), size),
                (p, lift, palette, tier_only),
                run,
                reach,
                builder,
            );
        }
        Primitive::Ring { quad } => {
            write_quad(
                quad,
                (sprites::RING, basic),
                (p, lift, palette, tier_only),
                run,
                reach,
                builder,
            );
        }
        Primitive::Burst {
            quad,
            on_surface: false,
        } => {
            let size = Vec2::splat(basic.max_element());
            write_quad(
                quad,
                (sprites::BURST, size),
                (p, lift, palette, tier_only),
                run,
                reach,
                builder,
            );
        }
        Primitive::Directed { sprite } => {
            write_directed(sprite, (p, lift), tier_only, run, reach, builder);
        }
        Primitive::Streak {
            lookback_ms,
            width,
            minimum,
            tint,
            opacity,
            record,
        } => {
            write_record(record.as_ref(), (p, lift), run, reach, builder);
            let before = u64::from(*lookback_ms) * 1_000_000;
            if let Some(was) = reach_at(before)
                && let Some(draw) = light::path_streak(
                    reach,
                    was,
                    (eye, lift),
                    minimum.at(p),
                    width.at(p),
                    light::fade(*tint, opacity.at(p)),
                )
            {
                builder.draw(Band::Glows, draw);
            }
        }
        Primitive::Vfx {
            vfx,
            size,
            tint,
            fade,
            offset,
            aspect,
        } => {
            if builder.tier() != Tier::Enhanced {
                return;
            }
            write_vfx(
                (*vfx, size, *tint, fade, *offset, *aspect),
                (p, lift, palette),
                run,
                reach,
                builder,
            );
        }
        _ => {}
    }
}

fn write_vfx(
    (kind, size, tint, fade, offset, aspect): (Vfx, &Scalar, Tint, &Scalar, Option<Vec2>, f32),
    (progress, lift, palette): (f32, f32, &EffectPalette),
    run: &EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    let opacity = (1.0 - fade.at(progress)).clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let card_width = if builder.card().x > 0.0 {
        builder.card().x * reach.pose.scale
    } else {
        reach.half.x * 2.0
    };
    let scale = if kind == Vfx::Settle {
        reach.settle.quad_scale()
    } else {
        1.0
    };
    let width = card_width * size.at(progress).max(0.0) * scale;
    if width <= 0.0 {
        return;
    }
    let height = width * aspect;
    let shift = (offset.unwrap_or(Vec2::ZERO) * card_width).extend(0.0);
    let position = light::above(&reach.pose, lift) + reach.pose.rotation * shift;
    let transform = glam::Affine3A::from_scale_rotation_translation(
        Vec3::new(width, height, 1.0),
        reach.pose.rotation,
        position,
    );
    let seed = super::vfx::unit_hash(run.particle_seed.unwrap_or(0) as usize, 0);
    let rgba = tint.resolve(palette, reach.colors);
    let style = if kind == Vfx::Settle {
        reach.settle as u8
    } else {
        0
    };
    builder.draw_tier_only(
        Band::Glows,
        Draw::vfx_styled(
            transform,
            rgba,
            kind,
            style,
            [progress, opacity, seed, aspect],
        ),
    );
}

/// Writes a burst in surface paint order, using the surface's own pose and scale.
pub(super) fn write_surface_burst(
    quad: &Quad,
    (progress, lift, palette): (f32, f32, &EffectPalette),
    tier_only: bool,
    run: &EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    if quad.fade.at(progress) < 1.0 {
        let size = Vec2::splat(reach.half.max_element() * 2.0);
        write_quad(
            quad,
            (sprites::BURST, size),
            (progress, lift, palette, tier_only),
            run,
            reach,
            builder,
        );
    }
}

fn write_record(
    record: Option<&LightRecord>,
    (progress, lift): (f32, f32),
    run: &EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    if let Some(record) = record {
        light::record_as(
            builder,
            run,
            (&reach.pose, lift),
            reach.half,
            (
                record.scale.at(progress),
                record.opacity.at(progress).clamp(0.0, 1.0),
            ),
        );
    }
}

fn write_quad(
    quad: &Quad,
    (sprite, size): (Sprite, Vec2),
    (progress, lift, palette, tier_only): (f32, f32, &EffectPalette, bool),
    run: &EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    let record = if run.on_point {
        quad.point_record.as_ref().or(quad.record.as_ref())
    } else {
        quad.record.as_ref()
    };
    write_record(record, (progress, lift), run, reach, builder);
    let tint = quad.tint.resolve(palette, reach.colors);
    let draw = light::lying(
        (&reach.pose, lift),
        size * quad.scale.at(progress),
        light::fade(tint, quad.alpha.at(progress)),
        sprite,
        quad.fade.at(progress).clamp(0.0, 1.0),
    );
    if tier_only {
        builder.draw_tier_only(Band::Glows, draw);
    } else {
        builder.draw(Band::Glows, draw);
    }
}

fn write_directed(
    directed: &DirectedSprite,
    (progress, lift): (f32, f32),
    tier_only: bool,
    run: &EffectRun,
    reach: Reach,
    builder: &mut FrameBuilder<'_>,
) {
    use std::f32::consts::{FRAC_PI_2, PI};

    let (axis, angle, along, across) = match directed.direction {
        Direction::Up => (Vec3::Y, 0.0, reach.half.y, reach.half.x),
        Direction::Left => (Vec3::NEG_X, FRAC_PI_2, reach.half.x, reach.half.y),
        Direction::Down => (Vec3::NEG_Y, PI, reach.half.y, reach.half.x),
        Direction::Right => (Vec3::X, -FRAC_PI_2, reach.half.x, reach.half.y),
    };
    let length = along * directed.length.at(progress);
    let width = 2.0 * across * directed.width.at(progress);
    let start = (directed.origin * reach.half).extend(0.0);
    let pose = reach.pose;
    let transform = glam::Affine3A::from_scale_rotation_translation(
        Vec3::new(width, length, 1.0),
        pose.rotation * Quat::from_rotation_z(angle),
        light::above(&pose, lift) + pose.rotation * start + pose.rotation * axis * (length * 0.5),
    );
    write_record(
        directed.record.as_ref(),
        (progress, lift),
        run,
        reach,
        builder,
    );
    let draw = kit_frame::frame::Draw::glow(
        transform,
        light::fade(directed.tint, directed.alpha.at(progress)),
        sprite(directed.shape),
        directed.fade.at(progress).clamp(0.0, 1.0),
    );
    if tier_only {
        builder.draw_tier_only(Band::Glows, draw);
    } else {
        builder.draw(Band::Glows, draw);
    }
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
            ..FloatLook::default()
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
pub fn text(run: &EffectRun, now: u64, tier: Tier, base: [u8; 4]) -> Option<TextLook> {
    let mut look = TextLook::default();
    for (primitive, p) in layers(run, now, tier) {
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
