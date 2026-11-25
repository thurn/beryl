//! The library, graveyard and exile piles (docs/beryl-3d.md, "Cards have real heights"): each
//! card one card `layer` (`card.json`) above the last, so that a 60-card library is 18 millimetres tall.
//!
//! - A pile draws few records however tall it is. Only its top card and the card under it are
//!   drawn as cards, the second so that it shows while the top one leaves. The cards beneath
//!   them keep their scene objects and resting poses but are not drawn (`Rest::visible` is
//!   false), so that a card taken from the middle of a pile starts where it lay. In their
//!   place at most fifteen flat records draw their edges as the web interface does: dark rims
//!   with light lines, spread to the right of the viewer's piles and below the opponent's
//!   (`write_side`).
//! - Each pile that holds a card has one resolved element, `Role::Pile`, named as the web
//!   interface's `pileName` names it ("Your library", "Opponent's exile"), whose corners are
//!   its top card's. The client shows its count as a float beside it (`hud::piles`).
//! - Where a pile lies is a point of the screen, copied from the web interface's `PILES`
//!   (`ui/src/game/pileLook.ts`), which keeps each side's piles in their corner of the window
//!   at any window shape. When a region fit supplies a projected anchor, that point is
//!   where the web draws the middle of the pile's top card, which stays there however many
//!   cards the pile holds: the pile stands wherever puts its top card under it, and its cards
//!   are as wide as the web's and lean back toward the camera until they are as tall. Otherwise
//!   (the phone layout) the pile's bottom card stands on the point of the table under it.

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, ZoneName};
use glam::{Affine3A, Mat3, Vec2, Vec3};

use super::place::{Placement, Rest, turn};
use super::scene::{Motion, SceneObject, Table};
use super::{Camera, Layout, Place};
use kit_base::data::Card;
use kit_base::data::profiles::PileGeometry;
use kit_base::pose::Pose;
use kit_base::tokens::size;
use kit_frame::frame::{
    Anchor, Band, Draw, ElementId, ElementLook, FrameBuilder, NameRef, REVEALED_FACE, Resolved,
    Role, StateBits,
};
use kit_space::arrange::Pile;

/// The element id of the first pile, the viewer's library: past every card's, every ability's
/// and every face's in the Revealed Cards pile. The six piles count up from it in the order of
/// `PileShown::index`.
pub const PILE_IDS: u32 = REVEALED_FACE + (1 << 16);

/// The zones a side has a pile for, in the order of their places in `Layout::piles`.
const ZONES: [ZoneName; 3] = [ZoneName::Library, ZoneName::Graveyard, ZoneName::Exile];

/// The colors of the edge of a card beneath a pile's top two, as the web interface's
/// `.pile-card-rim` (`ui/src/screens/styles/card-layer.css`) paints it: a dark fill, and a light
/// line of card stock on the side toward the table's middle.
const RIM_FILL: [u8; 4] = [0x21, 0x1b, 0x18, 255];
const RIM_LINE: [u8; 4] = [0xa3, 0x97, 0x87, 255];
/// The color of the opponent's rims' left edges, which face away from the light.
const RIM_SHADE: [u8; 4] = [0x6b, 0x61, 0x55, 255];

/// The deepest card beneath a pile's top card that the web interface draws.
const DEEPEST: usize = 63;

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
    /// How far its cards lean back from lying flat toward the camera, in radians.
    pub tilt: f32,
    /// How far its cards are sheared, as the web leans a pile's cards toward its vanishing
    /// point (`Pose::skew`).
    pub skew: f32,
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

    /// Where the pile's element and count stand, given `top`, where its top card rests: over
    /// that card while it lies on the pile, and over the place it would lie in while it lies
    /// beside the pile, as the cards the opponent looks at from the top of a library do
    /// (`table::lifted`), so that the library's element and count stay over the pile.
    fn shown_top(&self, top: Pose, card: Card, tolerance: f32) -> Pose {
        let layer = self.count.max(1) - 1;
        let base = bottom_pose(card, self.base, self.scale, self.tilt, self.skew);
        let on_pile = Pile::vertical(card).pose(base, layer);
        if top.position.truncate().distance(self.base) <= tolerance {
            top
        } else {
            Pose {
                turn: top.turn,
                ..on_pile
            }
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

/// Fits the pile's anchor and facing. A fitted pile keeps its top card at the web's
/// screen position and size. Its bottom moves, and its cards shrink slightly as the
/// top rises toward the eye. Otherwise the pile starts at the table point under its slot.
fn fit(
    layout: &Layout,
    camera: &Camera,
    (side, zone): (Side, ZoneName),
    (count, top): (usize, Option<ObjectId>),
) -> PileShown {
    let index = PileShown::index(side, zone);
    let (side_index, zone_index) = (index / 3, index % 3);
    let nominal = nominal_scale(layout, side);
    let mut pile = PileShown {
        count,
        top,
        scale: nominal,
        ..PileShown::default()
    };
    let Some(fitted) = layout.fitted_piles else {
        pile.base = camera.on_table(layout.piles[side_index][zone_index]);
        return pile;
    };

    pile.tilt = fitted.tilt[side_index];
    pile.skew = fitted.skew[side_index][zone_index];
    let at = fitted.tops[side_index][zone_index];
    let bottom = Pile::vertical(camera.data.card).fit_top(camera, at, count, nominal, pile.tilt);
    pile.scale = bottom.scale;
    pile.base = bottom.position.truncate();
    pile
}

/// The bottom card's pose, with the pile's painted shear.
fn bottom_pose(card: Card, base: Vec2, scale: f32, tilt: f32, skew: f32) -> Pose {
    Pose {
        skew,
        ..Pile::bottom(card, base, scale, tilt)
    }
}

/// How large `side`'s pile cards are against a card's printed size.
pub(super) fn nominal_scale(layout: &Layout, side: Side) -> f32 {
    match side {
        Side::You => layout.pile_scale[0],
        Side::Opponent => layout.pile_scale[1],
    }
}

/// Places a library, graveyard or exile pile, `objects` bottom first. Each card rests
/// one card `layer` above the last, and only the top two are drawn. A card lies
/// face up where its face is known to the viewer, as a known top card shows face up on its
/// library (UI Conventions, "Known cards in a library").
///
/// When the layout is fitted to the web, the card just below the top follows the same
/// screen offset as the painted rim, so the two line up while the top card leaves.
pub(super) fn lay_pile(
    placement: &mut Placement,
    objects: &[&ObjectView],
    (side, zone): (Side, ZoneName),
    (layout, camera): (&Layout, &Camera),
) {
    let count = objects.len();
    let pile = fit(
        layout,
        camera,
        (side, zone),
        (count, objects.last().map(|object| object.slot)),
    );
    let card = camera.data.card;
    let layers = Pile::vertical(card);
    let bottom = bottom_pose(card, pile.base, pile.scale, pile.tilt, pile.skew);
    let rim_scale = layout.fitted_piles.map(|fit| fit.rim_scale);
    for (height, object) in objects.iter().enumerate() {
        let mut pose = Pose {
            turn: turn(object),
            ..layers.pose(bottom, height)
        };
        if let Some(rim_scale) = rim_scale
            && height + 2 == count
        {
            let top = layers.pose(bottom, count - 1);
            let offset = rim_offset(&layout.pile, side, 1.0) * rim_scale;
            let point = camera.to_screen(top.position) + offset;
            pose.position = layer_centre(camera, top, point, card.layer);
        }
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
    placement.piles[PileShown::index(side, zone)] = pile;
}

/// The cards each pile shows at one moment of a script (`Table::piles_now`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PilesNow {
    /// How many of each pile's cards at rest, in `PileShown::index` order, are off it.
    left: [usize; 6],
    /// The slots of the top two cards still on each pile, top first, with their heights.
    top: [[Option<(usize, f32)>; 2]; 6],
    /// Whether `top` was worked out: at rest it is not, and every card is drawn as it rests.
    worked_out: bool,
}

impl PilesNow {
    /// Whether the card in `slot` is one of the top two cards still on its pile. A card that
    /// rests undrawn beneath a pile's top two is drawn once the cards above it leave, as one
    /// after another leaves when a spell draws or mills several: otherwise the pile shows only
    /// the dark rims `write_side` draws for the cards beneath, a black card where its top card
    /// should be.
    pub(super) fn shows(&self, slot: usize) -> bool {
        self.worked_out
            && self
                .top
                .iter()
                .flatten()
                .flatten()
                .any(|&(shown, _)| shown == slot)
    }
}

/// Whether a card that rests undrawn in a pile, `resting`, is drawn where the script leaves it,
/// `last`, in the same pile, as the cards a look lifts beside a library uncover the cards
/// beneath them. Such a card is drawn from the script's start, beneath the cards that still
/// cover it.
pub(super) fn uncovered(resting: SceneObject, last: Option<SceneObject>) -> bool {
    let in_pile = matches!(
        resting.place,
        Place::Card {
            zone: ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile,
            row: None,
            ..
        }
    );
    in_pile
        && !resting.visible
        && last.is_some_and(|last| last.visible && last.place == resting.place)
}

/// Places `side`'s exile, `objects` bottom first, on the phone layout: under that side's
/// graveyard, whose base is `at`, and never drawn, as the web's `ui/src/stage/phone.ts` places it
/// (docs/ui-conventions.md, "The phone"). The phone keeps the third pile's place in the
/// opponent's strip for the Revealed Cards pile, and lists exile in the graveyard's sheet. Each
/// card lies flat beneath the graveyard's bottom card, so a card flown into exile lands under
/// the graveyard rather than on it. The pile has no top card, so it writes no element and no
/// side record, though `PileShown::count` still counts its cards.
pub(super) fn lay_unseen_exile(
    placement: &mut Placement,
    objects: &[&ObjectView],
    side: Side,
    (layout, camera): (&Layout, &Camera),
) {
    let scale = nominal_scale(layout, side);
    let graveyard = PileShown::index(side, ZoneName::Graveyard);
    let at = camera.on_table(layout.piles[graveyard / 3][graveyard % 3]);
    let pile = Pile { step: Vec3::ZERO };
    let card = camera.data.card;
    let bottom = Pose {
        position: at.extend(card.layer * layout.pile.hidden_pile_layer),
        scale,
        ..Pose::default()
    };
    for (layer, object) in objects.iter().enumerate() {
        let pose = Pose {
            turn: turn(object),
            ..pile.pose(bottom, layer)
        };
        let place = Place::Card {
            zone: ZoneName::Exile,
            side,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: false,
                copies: 1,
            },
        );
    }
    placement.piles[PileShown::index(side, ZoneName::Exile)] = PileShown {
        count: objects.len(),
        base: at,
        top: None,
        scale,
        tilt: 0.0,
        skew: 0.0,
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
        // A prompt may hold the top card in a tray, or a step may already have moved it
        // elsewhere while the pile still holds its previous view's count. Match the pile
        // element's visibility rather than following that card with a second pile label.
        if self.in_box(top) || self.in_tray(top) || !self.lies_in(top, side, zone) {
            return None;
        }
        let top = pile.shown_top(
            self.rest(top)?,
            self.camera().data.card,
            self.layout().pile.top_tolerance,
        );
        Some((pile.count, self.camera().card_corners(top.transform())))
    }

    /// Anchors a count to the centre of the top card or the middle of its lowest edge.
    #[must_use]
    pub fn pile_count_anchor(&self, index: usize, gap: f32) -> Option<(usize, Anchor)> {
        let (side, zone) = PileShown::of_index(index);
        let (count, mut corners) = self.pile(side, zone)?;
        let low = corners.into_iter().reduce(Vec2::min)?;
        let high = corners.into_iter().reduce(Vec2::max)?;
        let (point, align, offset) = match side {
            Side::You => (
                corners[0]
                    .midpoint(corners[2])
                    .midpoint(corners[1].midpoint(corners[3])),
                Vec2::splat(0.5),
                Vec2::ZERO,
            ),
            Side::Opponent => {
                corners.sort_unstable_by(|a, b| a.y.total_cmp(&b.y));
                (
                    Vec2::new(f32::midpoint(corners[2].x, corners[3].x), corners[3].y),
                    Vec2::new(0.5, 0.0),
                    Vec2::new(0.0, gap),
                )
            }
        };
        let at = (point - low) / (high - low).max(Vec2::splat(f32::EPSILON));
        let id = ElementId(PILE_IDS + u32::try_from(index).ok()?);
        Some((count, Anchor::new(id, at, align, offset)))
    }

    /// Which cards each pile shows at this moment of `motion`, or at rest without one. A card is
    /// off its pile while a prompt's tray or a box holds it, or the tray or a box moves it at `now`,
    /// while it rests beside the pile, as
    /// the cards a look lifts do, while its track carries it, and once its track has begun when
    /// the script leaves it in another zone or beside the pile. Allocates nothing: the table
    /// works this out on every frame.
    pub(super) fn piles_now(&self, motion: Option<Motion<'_>>, now: u64) -> PilesNow {
        let mut piles_now = PilesNow::default();
        let tolerance = self.layout().pile.top_tolerance;
        let piles = &self.objects.piles;
        let lies_on = |object: &SceneObject, index: usize| {
            object.pose.position.truncate().distance(piles[index].base) <= tolerance
        };
        // At rest, with no tray up, no box and every pile's top card on its pile rather than
        // lifted beside it by a look, every card lies where `lay_pile` put it.
        let (tray, shown) = (
            self.held.is_some() || self.tray_moving(now),
            self.any_shown(),
        );
        let lifted = piles.iter().enumerate().any(|(index, pile)| {
            pile.top
                .and_then(|top| {
                    self.objects
                        .cards
                        .get(usize::from(top.0))
                        .copied()
                        .flatten()
                })
                .is_some_and(|top| !lies_on(&top, index))
        });
        if motion.is_none() && !tray && !shown && !lifted {
            return piles_now;
        }
        piles_now.worked_out = true;
        for (slot, resting) in self.objects.cards.iter().enumerate() {
            let Some(resting) = resting else {
                continue;
            };
            let Place::Card {
                zone: zone @ (ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile),
                side,
                row: None,
            } = resting.place
            else {
                continue;
            };
            // The phone keeps exile under the graveyard, never drawn (`lay_unseen_exile`).
            if !self.layout().pile_visible(zone) {
                continue;
            }
            let index = PileShown::index(side, zone);
            let off = !lies_on(resting, index);
            let leaves = off
                || motion.is_some_and(|motion| {
                    let last = motion.finals.cards.get(slot).copied().flatten();
                    let elsewhere = last
                        .is_none_or(|last| last.place != resting.place || !lies_on(&last, index));
                    motion.tracks.get(slot).is_some_and(|track| {
                        track.begun(motion.t) && (elsewhere || !track.settled(motion.t))
                    })
                });
            if leaves
                || (tray && (self.in_tray(slot) || self.tray_carries(slot, now)))
                || (shown && (self.in_box(slot) || self.shown_carries(slot, now)))
            {
                piles_now.left[index] += 1;
                continue;
            }
            // `lay_pile` rests each card one layer above the one below it.
            let height = resting.pose.position.z;
            let top = &mut piles_now.top[index];
            if top[0].is_none_or(|(_, above)| height > above) {
                top[1] = top[0];
                top[0] = Some((slot, height));
            } else if top[1].is_none_or(|(_, above)| height > above) {
                top[1] = Some((slot, height));
            }
        }
        piles_now
    }

    /// Writes each pile that holds a card: its resolved element, and the side of the cards
    /// beneath its top two, which `write` does not draw. While a script plays, the side shows
    /// only the cards that have not yet left the pile, and the element's `count` says how many
    /// those are: a script that takes every card of a pile, or the top cards one after another,
    /// would otherwise leave the dark rims of cards already gone with nothing on them, a black
    /// card where the pile's top card should be.
    pub(super) fn write_piles(&self, now: &PilesNow, builder: &mut FrameBuilder) {
        for (index, pile) in self.objects.piles.iter().enumerate() {
            let present = PileShown {
                count: pile.count.saturating_sub(now.left[index]),
                ..*pile
            };
            // A pile whose top card a box of the flat layer shows, as the viewer shows a pile's
            // cards, is drawn as though it had left.
            let (side, zone) = PileShown::of_index(index);
            let Some((slot, top)) = (pile.top)
                .map(|top| usize::from(top.0))
                .filter(|&top| !self.in_box(top))
                .and_then(|top| {
                    Some((
                        top,
                        pile.shown_top(
                            self.rest(top)?,
                            self.camera().data.card,
                            self.layout().pile.top_tolerance,
                        ),
                    ))
                })
            else {
                continue;
            };
            if self.layout().style.show_pile_sides {
                write_side(builder, (self.layout(), self.camera()), &present, side);
            }
            // A pile writes no element over its top card while a prompt holds that card up in
            // its tray, or once it has left the pile: a press, and the checks that ask what a
            // press reaches, must reach the card there.
            if self.in_tray(slot) || !self.lies_in(slot, side, zone) {
                continue;
            }
            let element = builder.resolved(
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
                    }
                    .into(),
                    pose: Some(top),
                    corners: self.camera().card_corners(top.transform()),
                    depth: self.camera().depth(top.position),
                    opacity: 1.0,
                    clip: None,
                    interactive: false,
                    state: StateBits(0),
                    face: None,
                    count: u16::try_from(present.count).unwrap_or(u16::MAX),
                    annotations: Default::default(),
                },
            );
            builder.look(
                ElementLook {
                    element,
                    background: None,
                    border: None,
                    radius: size::CARD_RADIUS,
                    color: [0; 4],
                    text: kit_ui::element::Style::default().text,
                },
                &[],
                &[],
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

/// Where the card `depth` below the top shows on the reference stage (y down). After a run of
/// full rim steps, the deeper cards close up. The viewer's piles spread right and the opponent's
/// spread down and left (`pileThickness` and `spread` in `ui/src/game/pileLook.ts`).
fn rim_offset(pile: &PileGeometry, side: Side, depth: f32) -> Vec2 {
    let spread = f32::from(pile.rim_spread_cards);
    let thickness = pile.rim_step * depth.min(spread)
        + pile.rim_curve_span * depth / (depth + pile.rim_curve_depth);
    match side {
        Side::You => Vec2::new(thickness, 0.0),
        Side::Opponent => pile.opponent_rim_direction * thickness,
    }
}

/// The centre under `point`, on a plane parallel to `top` and `below` millimetres behind it.
/// Using horizontal planes for the projected offsets can put a tilted rim at the same depth
/// as the top face. The depth test then draws triangles of the rim across the face.
fn layer_centre(camera: &Camera, top: Pose, point: Vec2, below: f32) -> Vec3 {
    let normal = top.rotation * Vec3::Z;
    let ray = camera.on_plane(point, top.position.z) - camera.eye;
    let distance = (top.position - camera.eye).dot(normal) - below;
    camera.eye + ray * (distance / ray.dot(normal))
}

/// One side of a pile's cards whose edges show beneath its top card, in the table's space.
struct Edge {
    /// The way out of the card across the edge.
    normal: Vec3,
    /// The way along the edge.
    along: Vec3,
    /// Half the card's size across the edge and along it.
    half_across: f32,
    half_along: f32,
    /// How wide the web's line along the edge is.
    line: f32,
    tint: [u8; 4],
    /// Whether the spread lines are drawn individually. The opponent's left
    /// edges step less than their line is wide, so they draw one band.
    stripes: bool,
    /// How far behind its card the edge is drawn, so that where two edges of one card
    /// meet at a corner, the light line covers the shaded one.
    below: f32,
}

/// Writes the side of `pile` beneath its top two cards as the web interface draws it: the edge
/// of each card beneath, its dark rim with a light line on the side toward the table's middle
/// (`.pile-card-rim` in `ui/src/screens/styles/card-layer.css`), spread out where the web's
/// `onPile` (`ui/src/game/pileLook.ts`) lays it. The viewer's pile shows its cards' right edges
/// to the right of the top card. The opponent's shows their near edges below it, and their
/// left edges, in a shaded line, to its left.
///
/// Each rim lies behind the two visible cards, under the point of the screen the web draws it at.
/// A rim's line shows only as far as the card above it leaves it uncovered, so a few records
/// draw what the web draws with one element a card:
///
/// - two dark records of the card mesh, under the shallowest and the deepest rim, whose union
///   is every rim's dark fill;
/// - a light stripe for each card at full rim spacing, as wide as its line
///   or the step to the card above it, whichever is narrower;
/// - one band for the rest, whose steps are narrower than a line, so that their lines meet: a
///   parallelogram swept by the edge from the first of them to the deepest.
///
/// The phone layout draws nothing beneath a pile's top card, as the web's `ui/src/stage/phone.ts`
/// draws none.
fn write_side(
    builder: &mut FrameBuilder,
    (layout, camera): (&Layout, &Camera),
    pile: &PileShown,
    side: Side,
) {
    // The web draws cards down to the 63rd beneath the top (`HIDDEN_DEPTH` in
    // `ui/src/game/layout.ts`), each but the first as a rim.
    let deepest = pile.count.saturating_sub(1).min(DEEPEST);
    if deepest < 2 {
        return;
    }
    let window = camera.window.size;
    let card = camera.data.card;
    let geometry = &layout.pile;
    let rim_scale = layout.fitted_piles.map_or_else(
        || Vec2::new(window.width, window.height) / geometry.rim_stage,
        |fit| fit.rim_scale,
    );
    let bottom = bottom_pose(card, pile.base, pile.scale, pile.tilt, pile.skew);
    let top = Pile::vertical(card).pose(bottom, pile.count - 1);
    // The rims lean back with the pile's cards (`PileShown::tilt`).
    let lean = top.rotation;
    let on_screen = camera.to_screen(top.position);
    #[expect(clippy::cast_precision_loss, reason = "depth is at most 63")]
    let centre = |depth: usize, below: f32| {
        let offset = rim_offset(geometry, side, depth as f32);
        layer_centre(camera, top, on_screen + offset * rim_scale, below)
    };
    #[expect(clippy::cast_precision_loss, reason = "depth is at most 63")]
    let separation = |depth: usize| card.layer * depth as f32;

    // The web shears each pile card about its own top left corner (`skewX`), which leans its
    // sides toward the vanishing point. The same shear about a card's middle, along the card's
    // own right and up, leans that card's rim.
    let (right_way, up_way) = (lean * Vec3::X, lean * Vec3::Y);
    let shear = |pivot: Vec3| {
        if pile.skew == 0.0 {
            return Affine3A::IDENTITY;
        }
        let linear = Mat3::IDENTITY
            + Mat3::from_cols(
                right_way * -pile.skew * up_way.x,
                right_way * -pile.skew * up_way.y,
                right_way * -pile.skew * up_way.z,
            );
        Affine3A {
            matrix3: linear.into(),
            translation: (pivot - linear * pivot).into(),
        }
    };
    let low = separation(deepest) + card.layer / 2.0;
    for depth in [2, deepest] {
        let middle = centre(depth, low);
        let transform = shear(middle)
            * Affine3A::from_scale_rotation_translation(
                Vec3::new(pile.scale, pile.scale, 1.0),
                lean,
                middle,
            );
        builder.draw(Band::Opaque, Draw::table(transform, RIM_FILL));
    }
    for edge in edges(card, geometry, side, pile.scale) {
        // The band's screen span can start at the second card, but its mesh must start
        // behind that card just like the individual stripes.
        let edge_centre = |depth: usize| centre(depth, separation(depth.max(2)) + edge.below);
        let normal = lean * edge.normal;
        let on_edge = |depth: usize| edge_centre(depth) + normal * edge.half_across;
        // The card mesh squeezed to a line lying flat, its middle at `middle`, spanning `across`
        // and the card's length along the edge. The squeezed mesh keeps the card's rounded
        // corners at the line's ends, as the web's border curves into the card's corners. The
        // same mesh as the dark records, rather than a quad, keeps one pile's records in one
        // batch of draw calls.
        let line = |pivot: Vec3, middle: Vec3, across: Vec3| {
            let along = lean * edge.along * 2.0 * edge.half_along;
            let (x, y) = if edge.along == Vec3::Y {
                (across, along)
            } else {
                (along, across)
            };
            let transform = Affine3A::from_cols(
                (x / camera.data.card.width).into(),
                (y / camera.data.card.height).into(),
                (lean * Vec3::Z).into(),
                middle.into(),
            );
            Draw::table(shear(pivot) * transform, edge.tint)
        };
        let mut band_from = 1;
        if edge.stripes {
            band_from = deepest.min(usize::from(geometry.rim_spread_cards)).max(1);
            for depth in 2..=band_from {
                let width = (on_edge(depth) - on_edge(depth - 1))
                    .dot(normal)
                    .clamp(0.0, edge.line);
                if width > 0.0 {
                    let middle = on_edge(depth) - normal * width / 2.0;
                    builder.draw(
                        Band::Opaque,
                        line(edge_centre(depth), middle, normal * width),
                    );
                }
            }
        }
        if deepest > band_from {
            // The parallelogram whose sides are the edges of the band's first and last cards.
            let (near, far) = (on_edge(band_from), on_edge(deepest));
            builder.draw(
                Band::Opaque,
                line(edge_centre(band_from), (near + far) / 2.0, far - near),
            );
        }
    }
}

/// The sides of `side`'s pile cards, `scale` times their printed size, whose edges show
/// beneath the top card. The web's lines are 2 pixels wide for the light one and 1 for the
/// shaded one, at the reference card width.
fn edges(
    card: Card,
    geometry: &PileGeometry,
    side: Side,
    scale: f32,
) -> impl Iterator<Item = Edge> {
    let (width, height) = (card.width * scale, card.height * scale);
    let pixel = width / geometry.rim_card_width;
    let right = Edge {
        normal: Vec3::X,
        along: Vec3::Y,
        half_across: width / 2.0,
        half_along: height / 2.0,
        line: 2.0 * pixel,
        tint: RIM_LINE,
        stripes: true,
        below: 0.0,
    };
    let near = Edge {
        normal: Vec3::NEG_Y,
        along: Vec3::X,
        half_across: height / 2.0,
        half_along: width / 2.0,
        ..right
    };
    let left = Edge {
        normal: Vec3::NEG_X,
        line: pixel,
        tint: RIM_SHADE,
        stripes: false,
        below: card.layer * geometry.rim_shade_layer,
        ..right
    };
    let edges = match side {
        Side::You => [Some(right), None],
        Side::Opponent => [Some(near), Some(left)],
    };
    edges.into_iter().flatten()
}
