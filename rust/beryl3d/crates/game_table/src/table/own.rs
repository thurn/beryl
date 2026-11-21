//! The marks a permanent's own record calls for, drawn as quads over its card
//! (docs/beryl-3d.md, "Objects and placement": marks are separate quads). They say what the
//! record says, so they need no span and no step: each is drawn while the record the table
//! draws the card from says so, and changes as the step that carries a new record begins.
//!
//! - Summoning sickness: a moon over the lower half of the card, as the web's `.card-sick-moon`
//!   leaves the art clear.
//! - A token: a coin at the bottom left, where the web's `.card-counters` row puts its label.
//! - A permanent its owner does not control: a violet outline, the web's `.card-stolen-edge`.
//! - An attacker: a wedge past the card's edge toward the defending side, its tip `chevron`
//!   card widths past the frame, as the web's `fighterLight` (`ui/src/stage/light.ts`) draws it
//!   with the desktop layout's `chevron`.
//! - A blocker: a crescent just past the card's edge toward the attacking side, as
//!   `fighterLight` draws it.
//!
//! The wedge and the crescent fade in over `fade_ms` from the frame the record first says the
//! creature fights. The wedge starts a little large and settles over `pop_ms`, and the crescent
//! opens from `opens_from` of its size over `open_ms`, as in `fighterLight`. The web keeps
//! these times in its code rather than in `data/ui-conventions.json`, so they are under `own`
//! in `data/beryl3d/space.json` with the sizes and offsets, which are shares of a card.

use std::cell::Cell;
use std::f32::consts::PI;

use beryl_protocol::protocol::{Combat, ObjectView, Side};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{CARD_HEIGHT, CARD_WIDTH, Look};
use kit_base::data::space::own;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, FrameBuilder, Sprite, StateBits, sprites};

/// How a creature fights, as its record says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fighting {
    #[default]
    No,
    Attacks,
    Blocks,
}

impl Fighting {
    fn of(record: &ObjectView) -> Self {
        match record.combat {
            Some(Combat::Attacking { .. }) => Self::Attacks,
            Some(Combat::Blocking { .. }) => Self::Blocks,
            None => Self::No,
        }
    }
}

/// For each slot, how its creature fought in the last frame that drew it and since when, in
/// nanoseconds since the client started. Drawing reads the table without changing it, so the
/// times are kept in cells. They are bookkeeping, not what the table shows, so two tables are
/// equal whatever their times.
#[derive(Clone, Debug)]
pub(super) struct Fights(Vec<Cell<(Fighting, u64)>>);

impl Fights {
    /// Room for `slots` slots, so that no frame allocates.
    pub(super) fn with_capacity(slots: usize) -> Self {
        Self(vec![Cell::new((Fighting::No, 0)); slots])
    }

    /// Since when the creature in `slot` has fought as `fighting`, noting `now` if it did not
    /// in the frame before. A slot past the room reserved is taken to have just begun.
    fn since(&self, slot: usize, fighting: Fighting, now: u64) -> u64 {
        let Some(cell) = self.0.get(slot) else {
            return now;
        };
        let (was, since) = cell.get();
        if was == fighting {
            return since;
        }
        cell.set((fighting, now));
        now
    }

    /// The slot holds no creature drawn in this frame.
    pub(super) fn forget(&self, slot: usize) {
        if let Some(cell) = self.0.get(slot) {
            cell.set((Fighting::No, 0));
        }
    }
}

impl PartialEq for Fights {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Writes the marks of `record`, the permanent in `slot` drawn as `look`, at `now`.
pub(super) fn write(
    (slot, record): (usize, &ObjectView),
    look: &Look,
    fights: &Fights,
    now: u64,
    builder: &mut FrameBuilder,
) {
    let fighting = Fighting::of(record);
    let since = fights.since(slot, fighting, now);
    if look.pose.opacity <= 0.0 {
        return;
    }
    let mut marks = Marks::on(look, builder);
    let card = Vec2::new(CARD_WIDTH, CARD_HEIGHT);
    if record.summoning_sick {
        let size = Vec2::splat(own::moon::SIZE * CARD_WIDTH);
        marks.icon(sprites::MOON, own::moon::AT * card, size, color::MOON);
    }
    if record.token {
        let size = Vec2::splat(own::coin::SIZE * CARD_WIDTH);
        marks.icon(sprites::COIN, own::coin::AT * card, size, color::COIN);
    }
    if record.controller != record.owner {
        let size = sprites::outline_size(card / 2.0);
        marks.icon(sprites::OUTLINE, Vec2::ZERO, size, color::OUTLINE);
    }
    if fighting != Fighting::No {
        #[expect(
            clippy::cast_precision_loss,
            reason = "an age in nanoseconds; a few seconds fit an f32's precision well enough"
        )]
        let age = now.saturating_sub(since) as f32 / 1e6;
        let edge = Edge::toward(look.pose.rotation, defending(record.controller));
        marks.fight(fighting, &edge, age);
    }
}

/// Writes the marks on one card: quads in the card's own space, lying just above its upper
/// side whether its face is up or down, in the band of marks on cards on the table or on cards
/// above it, as clear as the card is.
struct Marks<'b, 'f> {
    card: Affine3A,
    band: Band,
    opacity: f32,
    builder: &'b mut FrameBuilder<'f>,
}

impl<'b, 'f> Marks<'b, 'f> {
    fn on(look: &Look, builder: &'b mut FrameBuilder<'f>) -> Self {
        let pose = look.pose;
        Self {
            card: pose.upward(),
            band: if look.state & StateBits::MOVING == 0 {
                Band::TableMarks
            } else {
                Band::RaisedMarks
            },
            opacity: pose.opacity.clamp(0.0, 1.0),
            builder,
        }
    }

    /// The quad `size` large with its middle at `at`, turned by `turn` about the card's normal.
    fn place(&self, at: Vec2, size: Vec2, turn: f32) -> Affine3A {
        self.card
            * Affine3A::from_scale_rotation_translation(
                size.extend(1.0),
                Quat::from_rotation_z(turn),
                at.extend(own::LIFT),
            )
    }

    /// The quad `size` large with its middle `out` past the middle of `edge`, turned by `turn`
    /// about the card's normal. A card in a battlefield row leans back toward the camera, so
    /// past its near edge its plane runs under the table, which would hide the quad: there the
    /// quad is folded up about the edge until it lies level, as the web's mark lies flat on
    /// the stage past the card.
    fn past(&self, edge: &Edge, out: f32, size: Vec2, turn: f32) -> Affine3A {
        let axis = edge.axis.extend(0.0);
        let outward = self.card.transform_vector3(axis).normalize_or_zero();
        let normal = self.card.transform_vector3(Vec3::Z).normalize_or_zero();
        // Folding by `angle` toward the normal turns the outward way into
        // `outward * cos + normal * sin`, which is level where `tan = -outward.z / normal.z`.
        let fold = if outward.z < 0.0 && normal.z > f32::EPSILON {
            Quat::from_axis_angle(axis.cross(Vec3::Z), (-outward.z / normal.z).atan())
        } else {
            Quat::IDENTITY
        };
        self.card
            * Affine3A::from_rotation_translation(fold, (edge.axis * edge.reach).extend(own::LIFT))
            * Affine3A::from_scale_rotation_translation(
                size.extend(1.0),
                Quat::from_rotation_z(turn),
                (edge.axis * out).extend(0.0),
            )
    }

    fn icon(&mut self, sprite: Sprite, at: Vec2, size: Vec2, tint: [u8; 4]) {
        let draw = Draw::icon(
            self.place(at, size, 0.0),
            with_opacity(tint, self.opacity),
            sprite,
        );
        self.builder.draw(self.band, draw);
    }

    /// The wedge of an attacker or the crescent of a blocker on `edge`, `age` milliseconds
    /// after its record first said it fights. The wedge is two records over one quad, as the
    /// web's `fighterLight` lights it in two colors: its body, rim and halo in the ember glow,
    /// and its white-hot rim in the ember core.
    fn fight(&mut self, fighting: Fighting, edge: &Edge, age: f32) {
        let fade = (age / own::FADE_MS).clamp(0.0, 1.0);
        let turn = edge.axis.y.atan2(edge.axis.x) - PI / 2.0;
        if fighting == Fighting::Attacks {
            let pop = (-age / own::wedge::POP_MS).exp();
            let length = own::wedge::CHEVRON * CARD_WIDTH * (1.0 + own::wedge::POP.y * pop);
            let size = sprites::wedge_size(edge.across * (1.0 + own::wedge::POP.x * pop), length);
            let transform = self.past(edge, length / 2.0, size, turn);
            for (sprite, tint) in [
                (sprites::WEDGE, color::WEDGE),
                (sprites::WEDGE_HOT, color::WEDGE_HOT),
            ] {
                let tint = with_opacity(tint, fade * self.opacity);
                self.builder
                    .draw(self.band, Draw::glow(transform, tint, sprite, 0.0));
            }
            return;
        }
        let t = (age / own::crescent::OPEN_MS).clamp(0.0, 1.0);
        let open = own::crescent::OPENS_FROM
            + (1.0 - own::crescent::OPENS_FROM) * (1.0 - (1.0 - t).powi(3));
        let size = own::crescent::SIZE * edge.across * open;
        let out = own::crescent::GAP * edge.across;
        let draw = Draw::glow(
            self.past(edge, out, size, turn),
            with_opacity(color::CRESCENT, fade * self.opacity),
            sprites::CRESCENT,
            0.0,
        );
        self.builder.draw(self.band, draw);
    }
}

/// The way across the table, in the table's space, that a creature controlled by `controller`
/// faces as it fights: an attacker toward the players it attacks, and a blocker toward the
/// attacker it blocks, so both toward their controller's opponent. The viewer sits at -y.
fn defending(controller: Side) -> Vec2 {
    match controller {
        Side::You => Vec2::Y,
        Side::Opponent => -Vec2::Y,
    }
}

/// One edge of a card, in the card's own space: the way out of it (`axis`, one of its four
/// sides), how far its middle lies from the card's centre, and how long the edge is.
struct Edge {
    axis: Vec2,
    reach: f32,
    across: f32,
}

impl Edge {
    /// The edge of a card turned by `rotation` that faces most nearly `way`, a direction in the
    /// table's space. A tapped attacker's wedge is on its long side, which faces the defender.
    fn toward(rotation: Quat, way: Vec2) -> Self {
        let local = rotation.inverse() * way.extend(0.0);
        if local.y.abs() >= local.x.abs() {
            Self {
                axis: Vec2::new(0.0, local.y.signum()),
                reach: CARD_HEIGHT / 2.0,
                across: CARD_WIDTH,
            }
        } else {
            Self {
                axis: Vec2::new(local.x.signum(), 0.0),
                reach: CARD_WIDTH / 2.0,
                across: CARD_HEIGHT,
            }
        }
    }
}

/// `tint` with its opacity multiplied by `opacity`, from 0 to 1.
fn with_opacity([red, green, blue, alpha]: [u8; 4], opacity: f32) -> [u8; 4] {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of a byte, clamped to 0 to 255"
    )]
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}
