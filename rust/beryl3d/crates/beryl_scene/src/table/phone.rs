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
//!   about its middle at the web's size, 68 points wide, and reach over the creatures' rows by
//!   up to 25 points; the rows never move when a spell is cast and the camera stays where it
//!   is, so each creature keeps most of its height uncovered.
//! - Hands, piles, the stack and the players' anchors are placed relative to the camera, at
//!   the web's places in points, converted for each window when the layout is made
//!   (`layout`), never on the frame path.

use beryl_protocol::protocol::Side;
use glam::{Quat, Vec2, Vec3};

use super::{
    CARD_HEIGHT, CARD_WIDTH, Camera, CameraData, DESKTOP, HandData, Layout, PlayerAnchor, RowArea,
    Window,
};
use crate::data::space::layout::phone as data;
use crate::input::Rect;
use crate::pose::Pose;

/// A card's width over its height, the web interface's `RATIO`.
const RATIO: f32 = CARD_WIDTH / CARD_HEIGHT;

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

/// The phone layout's bands, top to bottom, which the game screen lays its controls in. The
/// strips and the hand band are the web interface's. The battlefield's bands are where the
/// camera shows each row, split halfway between neighbouring rows, so they follow the table at
/// any window size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhoneBands {
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

/// The phone layout's bands in `window`, or none for a window that takes the desktop layout.
/// The bands do not depend on what the view holds: the middle keeps room for the stack while
/// it is empty.
#[must_use]
pub fn phone_bands(window: Window) -> Option<PhoneBands> {
    window.phone().then(|| {
        bands(&Camera::fit_inside(
            camera_data(),
            window,
            battlefield(window),
        ))
    })
}

/// `side`'s strip in a phone `window`, the band of its piles and life total, without fitting
/// the camera: the strips stand at fixed distances from the window's edges, so the client can
/// place what stands in them on the frame path.
#[must_use]
pub fn strip(window: Window, side: Side) -> Band {
    match side {
        Side::You => {
            let bottom = window.size.height - hand_height(window);
            Band::between(bottom - data::bands::YOU_STRIP, bottom)
        }
        Side::Opponent => {
            let top = window.safe_area.top;
            Band::between(top, top + data::bands::OPPONENT_STRIP)
        }
    }
}

/// The phone layout's camera, as data.
fn camera_data() -> CameraData {
    CameraData {
        pitch_degrees: data::camera::PITCH_DEGREES,
        fov_degrees: data::camera::FOV_DEGREES,
        target: data::camera::TARGET,
        required: data::camera::REQUIRED,
    }
}

/// The left and right edges of the column, inside the safe area and the side gutters.
fn column(window: Window) -> (f32, f32) {
    let inset = window.safe_area;
    (
        inset.left + data::bands::SIDE,
        window.size.width - inset.right - data::bands::SIDE,
    )
}

/// A hand card's width in a column `width` points wide: a quarter of it, and at most 100
/// points (`handWidth`).
fn hand_width(width: f32) -> f32 {
    (width * data::bands::HAND_SHARE).min(data::bands::HAND_WIDTH)
}

/// The hand band's height: two thirds of a hand card and the bottom safe inset.
fn hand_height(window: Window) -> f32 {
    let (left, right) = column(window);
    (hand_width(right - left) / RATIO * data::bands::HAND_SHOWN).round() + window.safe_area.bottom
}

/// The part of `window` the battlefield fills, which the phone camera fits its required
/// rectangle inside: the column, from the foot of the opponent's strip to the head of the
/// viewer's.
#[must_use]
pub fn battlefield(window: Window) -> Rect {
    let (left, right) = column(window);
    let top = window.safe_area.top + data::bands::OPPONENT_STRIP;
    let bottom = window.size.height - hand_height(window) - data::bands::YOU_STRIP;
    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// The bands, seen through `camera`, the phone camera fitted to its window.
fn bands(camera: &Camera) -> PhoneBands {
    let window = camera.window;
    let field = camera.area;
    let on_screen = |y: f32| camera.to_screen(Vec3::new(0.0, y, 0.0)).y;
    let (creatures, back) = (-data::creatures::Y, -data::lands::Y);
    let half = CARD_HEIGHT / 2.0;
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
    let hand = hand_height(window);
    let (left, right) = column(window);
    PhoneBands {
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

/// Where the stack's items stand on the phone, in the middle band, as the web interface's
/// `anchors(...).stack` puts them: side by side about the column's middle, the top item at the
/// right and in front.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneStack {
    /// The middle of the band, in logical points from the window's top edge.
    pub middle: f32,
    /// An item's height on screen, in logical points.
    pub height: f32,
    /// How far apart the items' middles stand, across, and how far each row of a stack too
    /// long for one row stands above the row in front of it, in logical points.
    pub step: f32,
    /// The column's left and right edges, in logical points.
    pub left: f32,
    pub right: f32,
    /// The highest and lowest a stack in rows may reach: the top safe inset and the hand
    /// band's top, in logical points.
    pub top: f32,
    pub bottom: f32,
    /// How far in front of the eye the bottom item stands, and how much nearer each item stands
    /// than the one beneath it, in millimetres.
    pub depth: f32,
    pub step_depth: f32,
}

/// Where the item at `index`, bottom first, of a stack of `count` stands on the phone. The
/// items stand `step` apart, so each item beneath keeps a strip at least the phone's touch
/// target wide at its left. A stack too long for one row across the column stands in rows,
/// the top items in the front row and each row behind it `step` higher, so that every item
/// beneath keeps a square `step` on a side at its top left. The rows stand about the middle
/// band, over the creatures' rows, kept between `top` and `bottom`, and their items shrink if
/// even that is not room enough.
pub(super) fn stack_pose(index: usize, count: usize, data: &PhoneStack, camera: &Camera) -> Pose {
    let size = camera.window.size;
    let room = (data.right - data.left - data.height * RATIO).max(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the quotient is small and positive"
    )]
    let per_row = (room / data.step) as usize + 1;
    let rows = count.div_ceil(per_row);
    let row = index / per_row;
    let length = if row + 1 == rows {
        count - row * per_row
    } else {
        per_row
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (place, middle, behind, rise) = (
        (index % per_row) as f32,
        (length as f32 - 1.0) / 2.0,
        (rows - 1 - row) as f32,
        (rows - 1) as f32 * data.step,
    );
    let height = data.height.min(data.bottom - data.top - rise);
    // The front row's middle: the block of rows centred on the band, then moved down off the
    // top limit and up off the bottom one.
    let front = (data.middle + rise / 2.0)
        .max(data.top + rise + height / 2.0)
        .min(data.bottom - height / 2.0);
    let front = if rows == 1 { data.middle } else { front };
    let x = f32::midpoint(data.left, data.right) + (place - middle) * data.step;
    let y = front - behind * data.step;
    let ndc = ndc(camera.window, x, y);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let depth = data.depth - index as f32 * data.step_depth;
    camera.screen_pose(ndc, depth, Quat::IDENTITY, height / size.height)
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
fn piles(camera: &Camera, bands: &PhoneBands) -> ([[Vec2; 3]; 2], [f32; 2]) {
    let window = camera.window;
    let inset = window.safe_area;
    let pile_y = [
        bands.your_strip.middle(),
        bands
            .opponent_strip
            .middle()
            .max(inset.top + data::piles::opponent::BELOW_SAFE),
    ];
    let pile_x = [data::piles::you::X, data::piles::opponent::X];
    let pile_width = [data::piles::you::WIDTH, data::piles::opponent::WIDTH];
    let piles: [[Vec2; 3]; 2] = std::array::from_fn(|side| {
        let x = pile_x[side];
        [x.x, x.y, x.z].map(|x| ndc(window, inset.left + x, pile_y[side]))
    });
    let scale = std::array::from_fn(|side| {
        let base = camera.on_table(piles[side][0]).extend(0.0);
        pile_width[side] / (CARD_WIDTH * camera.points_per_millimetre(base))
    });
    (piles, scale)
}

/// The viewer's hand: cards a quarter of the column wide, at most 100 points, 4 points apart
/// while they have room, their top edges 8 points under the hand band's top (`placeHand`).
fn your_hand(window: Window, bands: &PhoneBands) -> HandData {
    let width = bands.right - bands.left;
    let wide = hand_width(width);
    HandData {
        depth: data::hand::DEPTH,
        step_depth: data::hand::STEP_DEPTH,
        height: wide / RATIO / window.size.height,
        edge: ndc(window, 0.0, bands.hand.top + data::bands::HAND_TOP).y,
        fan_radius: data::hand::FAN_RADIUS,
        spacing: (wide + data::hand::GAP) / wide,
        room: (width - wide) / wide,
        bias_degrees: data::hand::BIAS_DEGREES,
        across: ndc(window, f32::midpoint(bands.left, bands.right), 0.0).x,
        ceiling: ndc(window, 0.0, window.safe_area.top).y,
    }
}

/// The opponent's hand: card backs peeking 10 points over the top of their strip, at its right,
/// short of the menu (`backSpot`).
fn opponent_hand(window: Window, bands: &PhoneBands) -> HandData {
    let theirs = data::opponent_hand::WIDTH;
    let fan = theirs * (data::opponent_hand::ROOM + 1.0);
    let right = window.size.width - window.safe_area.right - data::opponent_hand::RIGHT;
    let foot = bands.opponent_strip.top - data::opponent_hand::RISE + theirs / RATIO;
    HandData {
        depth: data::opponent_hand::DEPTH,
        step_depth: data::opponent_hand::STEP_DEPTH,
        height: theirs / RATIO / window.size.height,
        edge: ndc(window, 0.0, foot).y,
        fan_radius: data::opponent_hand::FAN_RADIUS,
        spacing: data::opponent_hand::STEP / theirs,
        room: data::opponent_hand::ROOM,
        bias_degrees: 0.0,
        across: ndc(window, right - fan / 2.0, 0.0).x,
        ceiling: f32::INFINITY,
    }
}

/// The phone layout for `window`, whose hands, piles, stack and anchors are converted from the
/// web interface's places in points to this window's normalized device coordinates.
#[must_use]
pub fn layout(window: Window) -> Layout {
    let camera_data = camera_data();
    let camera = Camera::fit_inside(camera_data, window, battlefield(window));
    let bands = bands(&camera);
    let (piles, pile_scale) = piles(&camera, &bands);
    // Each life total in the middle of its player's strip; a phone shows no unspent mana, so a
    // streak out of a pool starts at the life total (`anchors` in `ui/src/stage/phone.ts`).
    let anchor = |strip: Band| {
        let life = ndc(window, window.size.width / 2.0, strip.middle());
        PlayerAnchor { life, pool: life }
    };
    let stack = PhoneStack {
        middle: bands.middle.middle(),
        height: data::stack::WIDTH / RATIO,
        step: data::stack::STEP,
        left: bands.left,
        right: bands.right,
        top: window.safe_area.top,
        bottom: bands.hand.top,
        depth: data::stack::DEPTH,
        step_depth: data::stack::STEP_DEPTH,
    };
    let row = |y: f32, left: f32, right: f32, centred: bool| RowArea {
        y,
        left,
        right,
        centred,
    };
    Layout {
        camera: camera_data,
        strip: data::STRIP,
        creatures: row(
            data::creatures::Y,
            data::creatures::LEFT,
            data::creatures::RIGHT,
            true,
        ),
        lands: row(data::lands::Y, data::lands::LEFT, data::lands::RIGHT, false),
        others: row(
            data::others::Y,
            data::others::LEFT,
            data::others::RIGHT,
            false,
        ),
        gap: data::GAP,
        pile_step: data::PILE_STEP,
        beneath_step: DESKTOP.beneath_step,
        piles,
        pile_scale,
        tray: DESKTOP.tray,
        stack: DESKTOP.stack,
        hand: your_hand(window, &bands),
        opponent_hand: opponent_hand(window, &bands),
        players: [anchor(bands.your_strip), anchor(bands.opponent_strip)],
        phone: Some(stack),
        bands: Some(bands),
    }
}
