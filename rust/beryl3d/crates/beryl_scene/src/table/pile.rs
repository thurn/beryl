//! The library, graveyard and exile piles (docs/beryl-3d.md, "Cards have real heights"): each
//! card one `LAYER` above the last, so that a 60-card library is 18 millimetres tall.
//!
//! - A pile draws few records however tall it is. Only its top card and the card under it are
//!   drawn as cards, the second so that it shows while the top one leaves. The cards beneath
//!   them keep their scene objects and resting poses but are not drawn (`Rest::visible` is
//!   false), so that a card taken from the middle of a pile starts where it lay. In their
//!   place one record of the card mesh, stood on its edge and scaled to their height, is the
//!   side of the pile that faces the camera, in the color of card stock.
//! - Each pile that holds a card has one resolved element, `Role::Pile`, named as the web
//!   interface's `pileName` names it ("Your library", "Opponent's exile"), whose corners are
//!   its top card's. The client shows its count as a float beside it (`hud::piles`).
//! - Where a pile lies is a point of the screen, copied from the web interface's `PILES`
//!   (`ui/src/game/pileLook.ts`), which keeps each side's piles in their corner of the window
//!   at any window shape. The pile stands on the point of the table under it.

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, ZoneName};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::place::{Placement, Rest, turn};
use super::scene::Table;
use super::{CARD_HEIGHT, Camera, LAYER, Layout};
use crate::frame::{
    Band, Draw, ElementId, FrameBuilder, NameRef, Place, REVEALED_FACE, Resolved, Role, StateBits,
};
use crate::pose::Pose;

/// The element id of the first pile, the viewer's library: past every card's, every ability's
/// and every face's in the Revealed Cards pile. The six piles count up from it in the order of
/// `PileShown::index`.
pub const PILE_IDS: u32 = REVEALED_FACE + (1 << 16);

/// The zones a side has a pile for, in the order of their places in `Layout::piles`.
const ZONES: [ZoneName; 3] = [ZoneName::Library, ZoneName::Graveyard, ZoneName::Exile];

/// The color of the side of a pile: the light card stock the web interface draws the edges of
/// a pile's cards in (`.pile-card-rim` in `ui/src/screens/styles/card-layer.css`).
const EDGE_TINT: [u8; 4] = [163, 151, 135, 255];

/// One pile as the table shows it at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PileShown {
    /// How many cards it holds.
    pub count: usize,
    /// The point of the table its bottom card's centre lies on.
    pub base: Vec2,
    /// The slot of its top card; none for an empty pile.
    pub top: Option<ObjectId>,
    /// How large its cards are against a card's printed size.
    pub scale: f32,
}

impl PileShown {
    /// The place of `side`'s pile of `zone` among the six: the viewer's library, graveyard and
    /// exile, then the opponent's.
    #[must_use]
    pub fn index(side: Side, zone: ZoneName) -> usize {
        let zone = ZONES.iter().position(|z| *z == zone).unwrap_or(2);
        match side {
            Side::You => zone,
            Side::Opponent => 3 + zone,
        }
    }

    /// The side and zone of the pile at `index`, the inverse of `index`.
    #[must_use]
    pub fn of_index(index: usize) -> (Side, ZoneName) {
        let side = if index < 3 { Side::You } else { Side::Opponent };
        (side, ZONES[index % 3])
    }
}

/// What a pile is called, as the web interface's `pileName` calls it: "Your library",
/// "Opponent's graveyard".
#[must_use]
pub fn pile_name(side: Side, zone: ZoneName) -> &'static str {
    match (side, zone) {
        (Side::You, ZoneName::Library) => "Your library",
        (Side::You, ZoneName::Graveyard) => "Your graveyard",
        (Side::You, _) => "Your exile",
        (Side::Opponent, ZoneName::Library) => "Opponent's library",
        (Side::Opponent, ZoneName::Graveyard) => "Opponent's graveyard",
        (Side::Opponent, _) => "Opponent's exile",
    }
}

/// The point of the table `side`'s pile of `zone` stands on, seen through `camera`.
pub(super) fn pile_base(layout: &Layout, camera: &Camera, side: Side, zone: ZoneName) -> Vec2 {
    let index = PileShown::index(side, zone);
    camera.on_table(layout.piles[index / 3][index % 3])
}

/// How large `side`'s pile cards are against a card's printed size.
pub(super) fn pile_scale(layout: &Layout, side: Side) -> f32 {
    match side {
        Side::You => layout.pile_scale[0],
        Side::Opponent => layout.pile_scale[1],
    }
}

/// Places a library, graveyard or exile pile, `objects` bottom first, on the point `at` of
/// the table, its cards `scale` times their printed size: each card one `LAYER` above the last,
/// and only the top two drawn. A card lies face up where its face is known to the viewer, as a
/// known top card shows face up on its library (UI Conventions, "Known cards in a library").
pub(super) fn lay_pile(
    placement: &mut Placement,
    objects: &[&ObjectView],
    (side, zone): (Side, ZoneName),
    (at, scale): (Vec2, f32),
) {
    let count = objects.len();
    for (height, object) in objects.iter().enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a pile holds a few hundred cards"
        )]
        let pose = Pose {
            position: at.extend(LAYER * (height as f32 + 0.5)),
            scale,
            turn: turn(object),
            ..Pose::default()
        };
        let place = Place::Card {
            zone,
            side,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: height + 2 >= count,
                copies: 1,
            },
        );
    }
    placement.piles[PileShown::index(side, zone)] = PileShown {
        count,
        base: at,
        top: objects.last().map(|object| object.slot),
        scale,
    };
}

impl Table {
    /// How many cards the pile numbered `index` (`PileShown::index`) holds as the table stands
    /// now, whether or not a box of the flat layer shows its top card.
    #[must_use]
    pub fn pile_count(&self, index: usize) -> usize {
        self.objects.piles[index].count
    }

    /// `side`'s pile of `zone` as the table shows it at rest: how many cards it holds and its
    /// top card's corners on screen, in logical points. None for an empty pile.
    #[must_use]
    pub fn pile(&self, side: Side, zone: ZoneName) -> Option<(usize, [Vec2; 4])> {
        let pile = self.objects.piles[PileShown::index(side, zone)];
        let top = usize::from(pile.top?.0);
        if self.in_box(top) {
            return None;
        }
        let top = self.rest(top)?;
        Some((pile.count, self.camera().card_corners(top.transform())))
    }

    /// Writes each pile that holds a card: its resolved element, and the side of the cards
    /// beneath its top two, which `write` does not draw.
    pub(super) fn write_piles(&self, builder: &mut FrameBuilder) {
        for (index, pile) in self.objects.piles.iter().enumerate() {
            // A pile whose top card a box of the flat layer shows, as the viewer shows a pile's
            // cards, is drawn as though it had left.
            let (side, zone) = PileShown::of_index(index);
            let Some((slot, top)) = (pile.top)
                .map(|top| usize::from(top.0))
                .filter(|&top| !self.in_box(top))
                .and_then(|top| Some((top, self.rest(top)?)))
            else {
                continue;
            };
            if let Some(draw) = side_record(pile) {
                builder.draw(Band::Opaque, draw);
            }
            // A pile writes no element over its top card while a prompt holds that card up in
            // its tray, or once it has left the pile: a press, and the checks that ask what a
            // press reaches, must reach the card there.
            if self.in_tray(slot) || !self.lies_in(slot, side, zone) {
                continue;
            }
            builder.resolved(
                pile_name(side, zone),
                Resolved {
                    id: ElementId(PILE_IDS + u32::try_from(index).expect("six piles")),
                    parent: u32::MAX,
                    role: Role::Pile,
                    name: NameRef::default(),
                    place: Place::Card {
                        zone,
                        side,
                        row: None,
                    },
                    corners: self.camera().card_corners(top.transform()),
                    depth: self.camera().depth(top.position),
                    opacity: 1.0,
                    clip: None,
                    interactive: false,
                    state: StateBits(0),
                    face: None,
                    copies: 1,
                    really: crate::frame::NameRef::default(),
                },
            );
        }
    }
}

impl Table {
    /// Whether the card in `slot` rests in `side`'s pile of `zone`.
    fn lies_in(&self, slot: usize, side: Side, zone: ZoneName) -> bool {
        let object = self.objects.cards.get(slot).copied().flatten();
        object.is_some_and(|object| {
            matches!(object.place, Place::Card { zone: z, side: s, .. } if z == zone && s == side)
        })
    }
}

/// The side of `pile` that faces the camera, under its top two cards: the card mesh stood on
/// its near edge and scaled to their height. None when the pile has no card under its top two.
#[must_use]
fn side_record(pile: &PileShown) -> Option<Draw> {
    let under = pile.count.checked_sub(2).filter(|under| *under > 0)?;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pile holds a few hundred cards"
    )]
    let height = LAYER * under as f32;
    // A quarter turn about x turns the mesh's face from up (+z) toward the viewer (-y), and its
    // height (y) upright.
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(pile.scale, height / CARD_HEIGHT, 1.0),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        Vec3::new(
            pile.base.x,
            pile.base.y - pile.scale * CARD_HEIGHT / 2.0,
            height / 2.0,
        ),
    );
    Some(Draw::table(transform, EDGE_TINT))
}
