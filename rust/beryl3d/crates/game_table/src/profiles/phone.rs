//! The phone layout of the table (docs/beryl-3d.md, "Space and camera" and "Sizes"): a window
//! whose shorter side is under 500 points shows the table as the web interface's phone layout
//! does (`phoneLayout` in `ui/src/mobile/layout.ts`), as one column of bands from the
//! opponent's strip at the top to the viewer's hand at the bottom.
//!
//! - The strips and the hand keep the web's sizes in logical points: the opponent's strip, 48
//!   points under the top safe inset, the viewer's strip, 60 points, and the hand band, two
//!   thirds of a hand card and the bottom safe inset. What lies between them is the
//!   battlefield.
//! - The camera keeps the desktop's field of view and looks down more steeply, 10 degrees from
//!   straight down against the desktop's 27, so that the far rows are not foreshortened below
//!   the touch target: at 375 by 647 a tapped card in the opponent's back row is 54 points
//!   tall, where a 44-point square needs 52 with the hit test's grid. Its distance is fitted so
//!   that the required rectangle of the table lies inside the battlefield (`battlefield`), and
//!   its target is chosen so that at 393 by 759, with the test safe area, the rectangle fills
//!   it to within 4 points on every edge.
//! - Each side has two battlefield rows, as on the desktop: the creatures nearest the middle,
//!   and behind them the lands from the left and the other permanents at the right. The web's
//!   phone layout gives the lands and the other permanents rows of their own, 29 and 42 points
//!   tall at 393 by 759, which is too small for the phone's 44-point touch target, so a full
//!   card there could not keep a pointable square.
//! - The middle band, between the two creatures' rows, is 50 millimetres of table: 44 points at
//!   375 by 647, as tall as the web's middle while the stack is empty. The stack's items stand
//!   about its middle and reach over the creatures' rows; the rows never move when a spell is
//!   cast and the camera stays where it is. An item keeps the web's size, 68 points wide, where
//!   the band is tall enough, as at 393 by 759 and 430 by 932, and is smaller where it is not
//!   (`stack_height`), 51 points wide at 375 by 647, so that a tapped creature, 56 points tall
//!   there, keeps an uncovered strip a press can reach. The items stand in one row, and a stack
//!   too long for it scrolls as a battlefield row does (`kit_space::arrange::Cascade`).
//! - Hands, piles, the stack and the players' anchors are placed relative to the camera, at
//!   the web's places in points, converted for each window when the layout is made
//!   (`layout`), never on the frame path.

use beryl_protocol::protocol::Side;
use glam::{Vec2, Vec3};

use super::row;
use crate::table::{
    Across, Camera, CentredStack, HandData, Layout, LifeDock, PlayerAnchor, Second, StackPlacement,
    TraySpace, Window,
};
use kit_base::data::Card;
use kit_base::data::profiles::{Banded, Bands, ProfileData, TrayTuning};
use kit_scene::hit::CELL;
use kit_ui::input::Rect;

/// Logical points per unit of life-orb art.
pub(super) const ORB_SCALE: f32 = kit_base::data::space::hud::phone::ORB_SCALE;

/// A card's width over its height, the web interface's `RATIO`.
fn ratio(card: Card) -> f32 {
    card.width / card.height
}

/// A band of the window, in logical points from its top edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    pub top: f32,
    pub height: f32,
}

impl Band {
    fn between(top: f32, bottom: f32) -> Self {
        Self {
            top,
            height: bottom - top,
        }
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.top + self.height
    }

    #[must_use]
    pub fn middle(self) -> f32 {
        self.top + self.height / 2.0
    }
}

/// The table's bands, top to bottom, which the game screen lays its controls in. The
/// strips and the hand band are the web interface's. The battlefield's bands are where the
/// camera shows each row, split halfway between neighbouring rows, so they follow the table at
/// any window size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TableBands {
    /// The opponent's piles, life total and hand.
    pub opponent_strip: Band,
    /// The opponent's lands and other permanents.
    pub opponent_back: Band,
    pub opponent_creatures: Band,
    /// Between the creatures' rows: the stack, and the phase while the stack is empty.
    pub middle: Band,
    pub your_creatures: Band,
    /// The viewer's lands and other permanents.
    pub your_back: Band,
    /// The viewer's piles and life total.
    pub your_strip: Band,
    /// The viewer's hand, which runs under the bottom safe inset and off the window's bottom.
    pub hand: Band,
    /// The left and right edges of the column every band spans, inside the side gutters.
    pub left: f32,
    pub right: f32,
}

pub(super) fn strip_with(window: Window, side: Side, (sizes, card): (Bands, Card)) -> Band {
    match side {
        Side::You => {
            let bottom = window.size.height - hand_height(window, sizes, card);
            Band::between(bottom - sizes.you_strip, bottom)
        }
        Side::Opponent => {
            let top = window.safe_area.top;
            Band::between(top, top + sizes.opponent_strip)
        }
    }
}

pub(super) fn column(window: Window, sizes: Bands) -> (f32, f32) {
    let inset = window.safe_area;
    (
        inset.left + sizes.side,
        window.size.width - inset.right - sizes.side,
    )
}

/// A hand card's width in the column.
fn hand_width(width: f32, sizes: Bands) -> f32 {
    (width * sizes.hand_share).min(sizes.hand_width)
}

/// The hand band's height includes the bottom safe inset.
fn hand_height(window: Window, sizes: Bands, card: Card) -> f32 {
    let (left, right) = column(window, sizes);
    (hand_width(right - left, sizes) / ratio(card) * sizes.hand_shown).round()
        + window.safe_area.bottom
}

pub(crate) fn battlefield_with(window: Window, sizes: Bands, card: Card) -> Rect {
    let (left, right) = column(window, sizes);
    let top = window.safe_area.top + sizes.opponent_strip;
    let bottom = window.size.height - hand_height(window, sizes, card) - sizes.you_strip;
    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

pub(crate) fn bands_with(
    camera: &Camera,
    sizes: Bands,
    creatures_y: f32,
    lands_y: f32,
) -> TableBands {
    let window = camera.window;
    let field = camera.area;
    let on_screen = |y: f32| camera.to_screen(Vec3::new(0.0, y, 0.0)).y;
    let (creatures, back) = (-creatures_y, -lands_y);
    let half = camera.data.card.height / 2.0;
    // Between the back row and the creatures' row, halfway between the near edge of the one and
    // the far edge of the other.
    let between = f32::midpoint(back - half, creatures + half);
    let lines = [
        field.y,
        on_screen(between),
        on_screen(creatures - half),
        on_screen(half - creatures),
        on_screen(-between),
        field.y + field.height,
    ];
    let hand = hand_height(window, sizes, camera.data.card);
    let (left, right) = column(window, sizes);
    TableBands {
        opponent_strip: Band::between(window.safe_area.top, lines[0]),
        opponent_back: Band::between(lines[0], lines[1]),
        opponent_creatures: Band::between(lines[1], lines[2]),
        middle: Band::between(lines[2], lines[3]),
        your_creatures: Band::between(lines[3], lines[4]),
        your_back: Band::between(lines[4], lines[5]),
        your_strip: Band::between(lines[5], window.size.height - hand),
        hand: Band::between(window.size.height - hand, window.size.height),
        left,
        right,
    }
}

/// How tall a stack item stands on the phone, in logical points, with its middle at `middle`:
/// the web interface's size, 68 points wide, unless that would leave a tapped creature in
/// either side's row less than a press can reach. Each creature must keep a strip the phone's
/// touch target tall and a cell of the hit test's grid more at each edge, between the item and
/// its far edge, so that the rows can stay where they are while a spell is on the stack.
fn stack_height(camera: &Camera, middle: f32, (width, row_y, target): (f32, f32, f32)) -> f32 {
    let uncovered = target + 2.0 * CELL;
    // A tapped card lies across its row, so its height on the table is a card's width.
    let half = camera.data.card.width / 2.0;
    let reach = [row_y, -row_y]
        .map(|row| {
            let far = [row - half, row + half]
                .map(|y| (camera.to_screen(Vec3::new(0.0, y, 0.0)).y - middle).abs())
                .into_iter()
                .fold(0.0, f32::max);
            far - uncovered
        })
        .into_iter()
        .fold(f32::INFINITY, f32::min);
    (width / ratio(camera.data.card)).min(2.0 * reach)
}

/// A point of `window`, in logical points from its top left corner, in normalized device
/// coordinates.
fn ndc(window: Window, x: f32, y: f32) -> Vec2 {
    let size = window.size;
    Vec2::new(x / size.width * 2.0 - 1.0, 1.0 - y / size.height * 2.0)
}

/// Where each pile lies and how large its cards are, by side, the viewer's first: in each
/// player's strip from its left, the library first, as the web's `pileSpot` puts them, the
/// opponent's clear of the top safe inset.
fn piles(camera: &Camera, bands: &TableBands, data: Banded) -> ([[Vec2; 3]; 2], [f32; 2]) {
    let window = camera.window;
    let inset = window.safe_area;
    let pile_y = [
        bands.your_strip.middle(),
        bands
            .opponent_strip
            .middle()
            .max(inset.top + data.piles.opponent.below_safe),
    ];
    let pile_x = [data.piles.you.x, data.piles.opponent.x];
    let pile_width = [data.piles.you.width, data.piles.opponent.width];
    let piles: [[Vec2; 3]; 2] = std::array::from_fn(|side| {
        let x = pile_x[side];
        [x.x, x.y, x.z].map(|x| ndc(window, inset.left + x, pile_y[side]))
    });
    let scale = std::array::from_fn(|side| {
        let base = camera.on_table(piles[side][0]).extend(0.0);
        pile_width[side] / (camera.data.card.width * camera.points_per_millimetre(base))
    });
    (piles, scale)
}

/// The highest the top edge of the viewer's hand may reach on a phone, in logical points from the
/// window's top: low enough that each card of the viewer's back row, tapped, keeps a strip the
/// phone's touch target tall above it, and a cell of the hit test's grid more at each edge. A
/// hand of several rows that rose higher would cover the lands and other permanents, and then
/// the creatures, past what a press can reach; the cards that do not fit under this ceiling
/// scroll instead.
fn hand_ceiling(camera: &Camera, lands_y: f32, target: f32) -> f32 {
    // A tapped card lies across its row, so its far edge is half a card's width from the row's
    // middle, toward the opponent.
    let far = camera
        .to_screen(Vec3::new(0.0, lands_y + camera.data.card.width / 2.0, 0.0))
        .y;
    far + target + 2.0 * CELL
}

/// The viewer's hand: cards a quarter of the column wide, at most 100 points, 4 points apart
/// while they have room, their top edges 8 points under the hand band's top (`placeHand`), and
/// in rows no higher than `ceiling` points from the window's top (`hand_ceiling`).
fn your_hand(
    window: Window,
    bands: &TableBands,
    ceiling: f32,
    (data, sizes, card): (Banded, Bands, Card),
) -> HandData {
    let width = bands.right - bands.left;
    let wide = hand_width(width, sizes);
    HandData {
        depth: data.hand.depth,
        step_depth: data.hand.step_depth,
        height: wide / ratio(card) / window.size.height,
        edge: ndc(window, 0.0, bands.hand.top + sizes.hand_top).y,
        fan_radius: data.hand.fan_radius,
        spacing: (wide + data.hand.gap) / wide,
        room: (width - wide) / wide,
        bias_degrees: data.hand.bias_degrees,
        across: ndc(window, f32::midpoint(bands.left, bands.right), 0.0).x,
        ceiling: ndc(window, 0.0, ceiling).y,
    }
}

/// The opponent's hand: card backs along the top of their strip, at its right, short of the menu
/// (`backSpot`). The web's backs peek 10 points over the strip's top, into the top safe inset,
/// where a card the viewer may see, such as one revealed from that hand, would stand under the
/// notch and the status bar, so here they stand 2 points under the strip's top
/// (`opponent_hand.rise` is -2), which keeps the raised corners of the fan's tilted cards
/// inside the safe area too.
fn opponent_hand(window: Window, bands: &TableBands, data: Banded, card: Card) -> HandData {
    let theirs = data.opponent_hand.width;
    let fan = theirs * (data.opponent_hand.room + 1.0);
    let right = window.size.width - window.safe_area.right - data.opponent_hand.right;
    let foot = bands.opponent_strip.top - data.opponent_hand.rise + theirs / ratio(card);
    HandData {
        depth: data.opponent_hand.depth,
        step_depth: data.opponent_hand.step_depth,
        height: theirs / ratio(card) / window.size.height,
        edge: ndc(window, 0.0, foot).y,
        fan_radius: data.opponent_hand.fan_radius,
        spacing: data.opponent_hand.step / theirs,
        room: data.opponent_hand.room,
        bias_degrees: 0.0,
        across: ndc(window, right - fan / 2.0, 0.0).x,
        ceiling: f32::INFINITY,
    }
}

/// The space the cards a prompt holds up lie in on a phone `window`, in logical points (a stage
/// as tall as the window), as the web's `phoneTray` gives it: across the column, over the
/// opponent's rows, and the second of two piles over the viewer's own, clear of the row of
/// buttons the prompt stands over the viewer's strip (`prompts::bar`), so that the middle band
/// and the phase in it stay clear. Its
/// cards are at most as wide as a card of the hand, and keep `least_step` apart, as the stack's
/// items do, so that each card beneath keeps a square of the touch target uncovered.
pub(super) fn tray_in(
    window: Window,
    bands: &TableBands,
    data: Banded,
    sizes: Bands,
    (tuning, card, grid_target): (TrayTuning, Card, f32),
) -> TraySpace {
    let shape = data.tray;
    let top = bands.opponent_back.top + shape.inset;
    let foot = bands.middle.bottom() + shape.inset;
    // The prompt's buttons stand on the dock's line while it holds cards up (`prompts::bar`):
    // the strip's middle, or over the top of the hand where it reaches higher. A hand with more
    // cards than one fan holds stands its back row in the strip, its top `HAND_TOP` over the
    // strip's top edge.
    let (button, gap) = (data.button_height, data.dock_gap);
    let hand_top = bands.your_strip.top - sizes.hand_top;
    let dock = bands.your_strip.middle().min(hand_top - gap - button / 2.0);
    let buttons = dock - button / 2.0 - gap;
    TraySpace {
        stage_height: window.size.height,
        y: top,
        height: bands.middle.top - shape.inset - top,
        across: Across::Column {
            left: bands.left,
            width: bands.right - bands.left,
        },
        second: Some(Second {
            y: foot,
            height: buttons - foot,
        }),
        most: shape.most,
        least: shape.least,
        gap: shape.gap,
        least_step: shape.least_step,
        grid_target,
        heading: shape.heading,
        tuning,
        card,
    }
}

/// A life total's anchor on a phone `window`, across the window's middle at `y` points from its
/// top. A phone shows no unspent mana, so a streak out of a pool starts at the
/// life total (`anchors` in `ui/src/stage/phone.ts`).
fn life_anchor(window: Window, y: f32) -> PlayerAnchor {
    let life = ndc(window, window.size.width / 2.0, y);
    PlayerAnchor { life, pool: life }
}

/// Fits the table inside the bands, with its stack between the creatures' rows.
pub(super) fn layout(profile: &ProfileData, window: Window, data: Banded) -> Layout {
    let table = &profile.table;
    let sizes = table.bands.expect("validated banded table");
    let area = battlefield_with(window, sizes, table.camera.card);
    let camera = Camera::fit_inside(table.camera, window, area);
    let bands = bands_with(&camera, sizes, table.creatures.y, table.lands.y);
    let (piles, pile_scale) = piles(&camera, &bands, data);
    let target = profile.checks.target_points;
    let middle = bands.middle.middle();
    let stack = CentredStack {
        middle,
        height: stack_height(
            &camera,
            middle,
            (data.stack.width, table.creatures.y, target),
        ),
        step: data.stack.step,
        left: bands.left,
        right: bands.right,
        count_room: data.stack.count_room,
        depth: data.stack.depth,
        step_depth: data.stack.step_depth,
    };
    let anchor = |strip: Band| life_anchor(window, strip.middle());
    Layout {
        camera: table.camera,
        camera_area: Some(area),
        strip: table.strip,
        creatures: row(table.creatures, true),
        lands: row(table.lands, false),
        others: row(table.others, false),
        gap: table.gap,
        pile_step: table.pile_step,
        beneath_step: table.beneath_step,
        piles,
        pile_scale,
        tray: tray_in(
            window,
            &bands,
            data,
            sizes,
            (table.tray_tuning, table.camera.card, target),
        ),
        stack: StackPlacement::Centred(stack),
        hand: your_hand(
            window,
            &bands,
            hand_ceiling(&camera, table.lands.y, target),
            (data, sizes, table.camera.card),
        ),
        opponent_hand: opponent_hand(window, &bands, data, table.camera.card),
        players: [anchor(bands.your_strip), anchor(bands.opponent_strip)],
        life_dock: Some(LifeDock {
            gap: data.dock_gap,
            middle: bands.your_strip.middle(),
            top: bands.your_back.top,
        }),
        revealed: Some((
            piles[1][2],
            data.piles.opponent.width / table.camera.card.width * table.camera.card.height
                / window.size.height,
        )),
        style: profile.table_style,
        play: profile.play,
        target,
        bands: Some(bands),
        row_zoom: None,
        fitted: None,
        fitted_piles: None,
    }
}
