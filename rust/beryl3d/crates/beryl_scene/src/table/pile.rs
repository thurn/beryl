//! The library, graveyard and exile piles (docs/beryl-3d.md, "Cards have real heights"): each
//! card one `LAYER` above the last, so that a 60-card library is 18 millimetres tall.
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
//!   at any window shape. On the desktop fitted to a window (`web_fit::piles`), that point is
//!   where the web draws the middle of the pile's top card, which stays there however many
//!   cards the pile holds: the pile stands wherever puts its top card under it, and its cards
//!   are as wide as the web's and lean back toward the camera until they are as tall. Otherwise
//!   (the phone layout) the pile's bottom card stands on the point of the table under it.

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, ZoneName};
use glam::{Affine3A, Mat3, Quat, Vec2, Vec3};

use super::place::{Placement, Rest, turn};
use super::scene::Table;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, LAYER, Layout};
use crate::frame::{
    Band, Draw, ElementId, ElementLook, FrameBuilder, NameRef, Place, REVEALED_FACE, Resolved,
    Role, StateBits,
};
use crate::pose::Pose;

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

/// The corner radius of a pile's element in logical points: the web's `--radius-card`
/// (`data/design-tokens.json`), which its `.pile-button` takes. The element draws nothing of its
/// own, so only the records `just b3d match` compares carry it.
const PILE_RADIUS: f32 = 7.0;

/// How many cards beneath a pile's top card each step a whole 1.25 stage pixels from the card
/// above, before the deeper ones close up (`pileThickness` in `ui/src/game/pileLook.ts`).
const SPREAD_CARDS: usize = 12;

/// The deepest card beneath a pile's top card that the web interface draws.
const DEEPEST: usize = 63;

/// How far from the point its pile stands on a top card may lie and still lie on the pile, in
/// millimetres: more than the pile moves as cards join or leave it within one script, and far
/// less than the cards lifted beside a library lie from it.
const ON_PILE: f32 = 4.0;

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
    #[must_use]
    pub fn shown_top(&self, top: Pose) -> Pose {
        let layer = self.count.max(1) - 1;
        let on_pile = layer_pose((self.base, self.scale, self.tilt, self.skew), layer);
        if top.position.truncate().distance(self.base) <= ON_PILE {
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

/// The point of the table `side`'s pile of `zone` stands on when it holds `count` cards, seen
/// through `camera`: on the desktop fitted to a window, the point under its top card's middle,
/// which `Layout::fitted_piles` places on the window at the height that card lies at.
pub(super) fn pile_base(
    layout: &Layout,
    camera: &Camera,
    (side, zone): (Side, ZoneName),
    count: usize,
) -> Vec2 {
    let index = PileShown::index(side, zone);
    let Some(fitted) = layout.fitted_piles else {
        return camera.on_table(layout.piles[index / 3][index % 3]);
    };
    let (scale, tilt) = (
        pile_card_scale(layout, camera, (side, zone), count),
        pile_tilt(layout, side),
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pile holds a few hundred cards"
    )]
    let top = LAYER * (count.max(1) as f32 - 0.5) + lift(scale, tilt);
    camera
        .on_plane(fitted.tops[index / 3][index % 3], top)
        .truncate()
}

/// How large the cards of `side`'s pile of `zone` are against a card's printed size when it
/// holds `count` cards. On the desktop fitted to a window, the web draws every pile's top card
/// the same size however many cards lie beneath it, but a tall pile's top card stands nearer the
/// camera and would be drawn larger, so its cards are scaled down by as much as the camera's
/// distance to them shrank. Elsewhere it is `pile_scale`.
pub(super) fn pile_card_scale(
    layout: &Layout,
    camera: &Camera,
    (side, zone): (Side, ZoneName),
    count: usize,
) -> f32 {
    let nominal = pile_scale(layout, side);
    let Some(fitted) = layout.fitted_piles else {
        return nominal;
    };
    let index = PileShown::index(side, zone);
    let (at, tilt) = (fitted.tops[index / 3][index % 3], pile_tilt(layout, side));
    // How far the middle of a pile card `layers` above the table, its cards `scale` large and
    // under the point `at` of the window, stands from the eye.
    let distance = |layers: f32, scale: f32| {
        let height = LAYER * layers + lift(scale, tilt);
        camera.depth(camera.on_plane(at, height).truncate().extend(height))
    };
    let reference = distance(0.5, nominal);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pile holds a few hundred cards"
    )]
    let layers = count.max(1) as f32 - 0.5;
    // The card's height above the table depends on its size, so the sum is repeated.
    let mut scale = nominal;
    for _ in 0..3 {
        scale = nominal * distance(layers, scale) / reference;
    }
    scale
}

/// How far `side`'s pile cards lean back from lying flat toward the camera, in radians: none
/// unless the layout is fitted to a window.
pub(super) fn pile_tilt(layout: &Layout, side: Side) -> f32 {
    layout.fitted_piles.map_or(0.0, |fitted| {
        fitted.tilt[usize::from(side == Side::Opponent)]
    })
}

/// How far a pile card `scale` times its printed size and leaning back by `tilt` stands above
/// where it would lie flat, so that its near edge stays where a flat card's lies.
pub(super) fn lift(scale: f32, tilt: f32) -> f32 {
    CARD_HEIGHT / 2.0 * scale * tilt.sin()
}

/// How far `side`'s pile of `zone` shears its cards: the tangent of the web's `lean` of its top
/// card (`pileLook` in `ui/src/game/pileLook.ts`), which is how far the card's foot lies to the
/// right of its top for each unit of height. None unless the layout is fitted to a window.
pub(super) fn pile_skew(layout: &Layout, (side, zone): (Side, ZoneName)) -> f32 {
    let index = PileShown::index(side, zone);
    layout
        .fitted_piles
        .map_or(0.0, |fitted| fitted.skew[index / 3][index % 3])
}

/// The pose, face up, of the card `layer` cards above the bottom of a pile that stands on the
/// point `base` of the table, its cards `scale` times their printed size, leaning back by `tilt`
/// radians and sheared by `skew`.
fn layer_pose((base, scale, tilt, skew): (Vec2, f32, f32, f32), layer: usize) -> Pose {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pile holds a few hundred cards"
    )]
    let height = LAYER * (layer as f32 + 0.5);
    Pose {
        position: base.extend(height + lift(scale, tilt)),
        rotation: Quat::from_rotation_x(tilt),
        scale,
        skew,
        ..Pose::default()
    }
}

/// How large `side`'s pile cards are against a card's printed size.
pub(super) fn pile_scale(layout: &Layout, side: Side) -> f32 {
    match side {
        Side::You => layout.pile_scale[0],
        Side::Opponent => layout.pile_scale[1],
    }
}

/// Places a library, graveyard or exile pile, `objects` bottom first, on the point `at` of
/// the table, its cards `scale` times their printed size and leaning back toward the camera by
/// `tilt` radians: each card one `LAYER` above the last, and only the top two drawn. A card lies
/// face up where its face is known to the viewer, as a known top card shows face up on its
/// library (UI Conventions, "Known cards in a library").
///
/// With a `camera`, which the desktop fitted to a window passes, the card under the top one
/// lies where the web draws it, `thickness(1)` stage pixels along `spread` from the top card,
/// as the rims beneath it do (`write_side`).
pub(super) fn lay_pile(
    placement: &mut Placement,
    objects: &[&ObjectView],
    (side, zone): (Side, ZoneName),
    (at, scale, tilt, skew): (Vec2, f32, f32, f32),
    camera: Option<&Camera>,
) {
    let count = objects.len();
    for (height, object) in objects.iter().enumerate() {
        let mut pose = Pose {
            turn: turn(object),
            ..layer_pose((at, scale, tilt, skew), height)
        };
        if let (Some(camera), true) = (camera, height + 2 == count) {
            let top = layer_pose((at, scale, tilt, skew), count - 1).position;
            let window = camera.window.size;
            let offset = spread(side, thickness(1.0))
                * Vec2::new(window.width / 1600.0, window.height / 900.0);
            let point = camera.to_screen(top) + offset;
            let z = pose.position.z;
            pose.position = camera.on_plane(point, z).truncate().extend(z);
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
    placement.piles[PileShown::index(side, zone)] = PileShown {
        count,
        base: at,
        top: objects.last().map(|object| object.slot),
        scale,
        tilt,
        skew,
    };
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
    (at, scale): (Vec2, f32),
) {
    for object in objects {
        let pose = Pose {
            position: at.extend(LAYER * 0.25),
            scale,
            turn: turn(object),
            ..Pose::default()
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
        let top = pile.shown_top(self.rest(top)?);
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
                .and_then(|top| Some((top, pile.shown_top(self.rest(top)?))))
            else {
                continue;
            };
            if self.layout().phone.is_none() {
                write_side(builder, self.camera(), pile, side);
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
                    note: crate::frame::NameRef::default(),
                },
            );
            builder.look(
                ElementLook {
                    element,
                    background: None,
                    border: None,
                    radius: PILE_RADIUS,
                    color: [0; 4],
                    text: crate::element::Style::default().text,
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

/// The web interface's `pileThickness` (`ui/src/game/pileLook.ts`): how far, in pixels of its
/// 1600 by 900 stage, the card `depth` beneath a pile's top card lies from it. Each of the first
/// `SPREAD_CARDS` steps 1.25 pixels, and the deeper ones close up.
fn thickness(depth: f32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "a small constant")]
    let spread = SPREAD_CARDS as f32;
    1.25 * depth.min(spread) + 13.0 * depth / (depth + 45.0)
}

/// The web interface's `spread`: which way, on its stage, a layer `thickness` beneath `side`'s
/// pile lies from the top card (y down). The viewer's piles spread right, the opponent's down
/// and a little left.
fn spread(side: Side, thickness: f32) -> Vec2 {
    match side {
        Side::You => Vec2::new(thickness, 0.0),
        Side::Opponent => Vec2::new(-0.3 * thickness, 0.65 * thickness),
    }
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
    /// Whether each of the first `SPREAD_CARDS` lines is drawn on its own. The opponent's left
    /// edges step less than their line is wide, so they draw one band.
    stripes: bool,
    /// How far below its card's height the edge is drawn, so that where two edges of one card
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
/// Each rim lies flat at its card's height, under the point of the screen the web draws it at.
/// A rim's line shows only as far as the card above it leaves it uncovered, so a few records
/// draw what the web draws with one element a card:
///
/// - two dark records of the card mesh, under the shallowest and the deepest rim, whose union
///   is every rim's dark fill;
/// - a light stripe for each of the first `SPREAD_CARDS` cards beneath, as wide as its line
///   or the step to the card above it, whichever is narrower;
/// - one band for the rest, whose steps are narrower than a line, so that their lines meet: a
///   parallelogram swept by the edge from the first of them to the deepest.
///
/// The phone layout draws nothing beneath a pile's top card, as the web's `ui/src/stage/phone.ts`
/// draws none.
fn write_side(builder: &mut FrameBuilder, camera: &Camera, pile: &PileShown, side: Side) {
    // The web draws cards down to the 63rd beneath the top (`HIDDEN_DEPTH` in
    // `ui/src/game/layout.ts`), each but the first as a rim.
    let deepest = pile.count.saturating_sub(1).min(DEEPEST);
    if deepest < 2 {
        return;
    }
    let window = camera.window.size;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a pile holds a few hundred cards"
    )]
    let top_height = LAYER * (pile.count as f32 - 0.5) + lift(pile.scale, pile.tilt);
    // The rims lean back with the pile's cards (`PileShown::tilt`).
    let lean = Quat::from_rotation_x(pile.tilt);
    let top = camera.to_screen(pile.base.extend(top_height));
    // The point of the table `height` up that lies under the web's card `depth` beneath the top.
    #[expect(clippy::cast_precision_loss, reason = "depth is at most 63")]
    let centre = |depth: usize, height: f32| {
        let offset = spread(side, thickness(depth as f32));
        let point = top + offset * Vec2::new(window.width / 1600.0, window.height / 900.0);
        camera.on_plane(point, height)
    };
    #[expect(clippy::cast_precision_loss, reason = "depth is at most 63")]
    let height = |depth: usize| top_height - LAYER * depth as f32;
    let at = |depth: usize| centre(depth, height(depth));

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
    let low = height(deepest) - LAYER / 2.0;
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
    for edge in edges(side, pile.scale) {
        // The middle of `depth`'s edge, drawn `edge.below` under its card.
        let normal = lean * edge.normal;
        let on_edge = |depth: usize| at(depth) + normal * edge.half_across - Vec3::Z * edge.below;
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
                (x / CARD_WIDTH).into(),
                (y / CARD_HEIGHT).into(),
                (lean * Vec3::Z).into(),
                middle.into(),
            );
            Draw::table(shear(pivot) * transform, edge.tint)
        };
        let mut band_from = 1;
        if edge.stripes {
            band_from = deepest.min(SPREAD_CARDS);
            for depth in 2..=band_from {
                let width = (on_edge(depth) - on_edge(depth - 1))
                    .dot(normal)
                    .clamp(0.0, edge.line);
                if width > 0.0 {
                    let middle = on_edge(depth) - normal * width / 2.0;
                    builder.draw(Band::Opaque, line(at(depth), middle, normal * width));
                }
            }
        }
        if deepest > band_from {
            // The parallelogram whose sides are the edges of the band's first and last cards.
            let (near, far) = (on_edge(band_from), on_edge(deepest));
            builder.draw(
                Band::Opaque,
                line(at(band_from), (near + far) / 2.0, far - near),
            );
        }
    }
}

/// The sides of `side`'s pile cards, `scale` times their printed size, whose edges show
/// beneath the top card. The web's lines are 2 pixels wide for the light one and 1 for the
/// shaded one, on a card drawn `FULL_WIDTH` (182) pixels wide.
fn edges(side: Side, scale: f32) -> impl Iterator<Item = Edge> {
    let (width, height) = (CARD_WIDTH * scale, CARD_HEIGHT * scale);
    let pixel = width / 182.0;
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
        below: LAYER / 4.0,
        ..right
    };
    let edges = match side {
        Side::You => [Some(right), None],
        Side::Opponent => [Some(near), Some(left)],
    };
    edges.into_iter().flatten()
}
