//! One edge of a card and the quads that lie past it, and the halo round a card shown away from
//! the zone it is in: geometry and light in a card's own space.

use std::f32::consts::{PI, TAU};

use glam::{Affine3A, Quat, Vec2, Vec3};
use kit_base::data::light::AwayHalo;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, FrameBuilder, sprites};

/// Writes the halo round a card shown away from the zone it is in, such as one a hand shows that
/// its player may cast from another zone, at `now` in nanoseconds since the client started: two
/// `HALO` glows in `band`, violet outside and pale inside, each breathing over
/// `halo.period_ms` out of step with the other, so that the light seems to drift round the card
/// like smoke. `card` is the card's own space, its face up, and `lift` how far over its face
/// the light lies.
pub fn write_away_halo(
    builder: &mut FrameBuilder<'_>,
    halo: &AwayHalo,
    (card, lift): (Affine3A, f32),
    (now, band, opacity): (u64, Band, f32),
) {
    let card_half = builder.card() / 2.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a time within an hour, in milliseconds, fits an f32's precision"
    )]
    let phase = (now / 1_000_000 % 3_600_000) as f32 / halo.period_ms * TAU;
    for (reach, tint, offset) in [
        (halo.reach, color::AWAY, 0.0),
        (halo.inner, color::AWAY_CORE, PI),
    ] {
        let size =
            sprites::halo_size(card_half * reach * (1.0 + halo.breath * (phase + offset).sin()));
        let transform = card
            * Affine3A::from_scale_rotation_translation(
                size.extend(1.0),
                Quat::IDENTITY,
                Vec3::Z * lift,
            );
        let draw = Draw::glow(transform, with_opacity(tint, opacity), sprites::HALO, 0.0);
        builder.draw(band, draw);
    }
}

/// The quad `size` large with its middle `out` past the middle of `edge`, turned by `turn` about
/// the card's normal. A card lying in a row on the table leans back toward the camera, so past its near
/// edge its plane runs under the table, which would hide the quad: there the quad is folded up
/// about the edge until it lies level, as the web's mark lies flat on the stage past the card.
pub fn past_edge(
    card: Affine3A,
    lift: f32,
    edge: &Edge,
    out: f32,
    size: Vec2,
    turn: f32,
) -> Affine3A {
    let axis = edge.axis.extend(0.0);
    let outward = card.transform_vector3(axis).normalize_or_zero();
    let normal = card.transform_vector3(Vec3::Z).normalize_or_zero();
    // Folding by `angle` toward the normal turns the outward way into
    // `outward * cos + normal * sin`, which is level where `tan = -outward.z / normal.z`.
    let fold = if outward.z < 0.0 && normal.z > f32::EPSILON {
        Quat::from_axis_angle(axis.cross(Vec3::Z), (-outward.z / normal.z).atan())
    } else {
        Quat::IDENTITY
    };
    card * Affine3A::from_rotation_translation(fold, (edge.axis * edge.reach).extend(lift))
        * Affine3A::from_scale_rotation_translation(
            size.extend(1.0),
            Quat::from_rotation_z(turn),
            (edge.axis * out).extend(0.0),
        )
}

/// One edge of a card, in the card's own space: the way out of it (`axis`, one of its four
/// sides), how far its middle lies from the card's centre, and how long the edge is.
pub struct Edge {
    pub axis: Vec2,
    pub reach: f32,
    pub across: f32,
}

impl Edge {
    /// The edge of a card turned by `rotation` that faces most nearly `way`, a direction in the
    /// table's space, on a card `card` wide and high. A tapped attacker's wedge is on its long
    /// side, which faces the defender.
    #[must_use]
    pub fn toward(rotation: Quat, way: Vec2, card: Vec2) -> Self {
        let local = rotation.inverse() * way.extend(0.0);
        if local.y.abs() >= local.x.abs() {
            Self {
                axis: Vec2::new(0.0, local.y.signum()),
                reach: card.y / 2.0,
                across: card.x,
            }
        } else {
            Self {
                axis: Vec2::new(local.x.signum(), 0.0),
                reach: card.x / 2.0,
                across: card.y,
            }
        }
    }
}

/// `tint` with its opacity multiplied by `opacity`, from 0 to 1.
#[must_use]
pub fn with_opacity([red, green, blue, alpha]: [u8; 4], opacity: f32) -> [u8; 4] {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a byte, clamped to 0 to 255"
    )]
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}
