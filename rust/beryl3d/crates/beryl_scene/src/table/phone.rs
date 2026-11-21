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
//!   too long for it scrolls as a battlefield row does (`stack_window`).
//! - Hands, piles, the stack and the players' anchors are placed relative to the camera, at
//!   the web's places in points, converted for each window when the layout is made
//!   (`layout`), never on the frame path.

use beryl_protocol::protocol::{Side, ZoneName};
use glam::{Quat, Vec2, Vec3};

use super::tray_layout::{Across, Second};
use super::{
    CARD_HEIGHT, CARD_WIDTH, Camera, CameraData, DESKTOP, HandData, Layout, PlayerAnchor, RowArea,
    RowWindow, TraySpace, Window,
};
use crate::agent::hit::CELL;
use crate::data::audit;
use crate::data::space::layout::phone as data;
use crate::frame::Place;
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
    /// How far apart the items' middles stand, across, in logical points.
    pub step: f32,
    /// The column's left and right edges, in logical points.
    pub left: f32,
    pub right: f32,
    /// The room kept at each end of a stack too long for one row, for the count of the items
    /// scrolled off that end, in logical points.
    pub count_room: f32,
    /// How far in front of the eye the bottom item stands, and how much nearer each item stands
    /// than the one beneath it, in millimetres.
    pub depth: f32,
    pub step_depth: f32,
}

impl PhoneStack {
    /// An item's width on screen, in logical points.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.height * RATIO
    }
}

/// Which items of a stack of `count` are in view on the phone, scrolled `from_top` items down
/// from its top. The items stand in one row across the column, `step` apart, so each item
/// beneath keeps a strip at least the phone's touch target wide at its left. A stack too long
/// for the row shows as many of its items as fit beside room at each end for the count of the
/// items off it, and scrolls as a battlefield row does: rows of items stacked over the
/// battlefield would cover the creatures' rows past what a press can reach.
#[must_use]
pub(super) fn stack_window(count: usize, from_top: usize, data: &PhoneStack) -> RowWindow {
    let fits = |room: f32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the quotient is small and positive"
        )]
        let more = (room.max(0.0) / data.step) as usize;
        more + 1
    };
    let room = data.right - data.left - data.width();
    let all = fits(room);
    let shown = if count <= all {
        count
    } else {
        fits(room - 2.0 * data.count_room)
    };
    let last = count - shown;
    let first = last - from_top.min(last);
    RowWindow::new(count, first, shown, (first, last - first))
}

/// Where the item at `index`, bottom first, of a stack whose `window` is in view stands on the
/// phone, and whether it is drawn: in view, side by side about the column's middle; out of
/// view, undrawn at the nearer end of the row.
pub(super) fn stack_pose(
    index: usize,
    window: RowWindow,
    data: &PhoneStack,
    camera: &Camera,
) -> (Pose, bool) {
    let size = camera.window.size;
    let (first, shown) = (window.first(), window.shown().max(1));
    let visible = (first..first + shown).contains(&index);
    let at = index.clamp(first, first + shown - 1) - first;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (place, middle, depth) = (
        at as f32,
        (shown as f32 - 1.0) / 2.0,
        data.depth - index as f32 * data.step_depth,
    );
    let x = f32::midpoint(data.left, data.right) + (place - middle) * data.step;
    let ndc = ndc(camera.window, x, data.middle);
    let pose = camera.screen_pose(ndc, depth, Quat::IDENTITY, data.height / size.height);
    (pose, visible)
}

/// How tall a stack item stands on the phone, in logical points, with its middle at `middle`:
/// the web interface's size, 68 points wide, unless that would leave a tapped creature in
/// either side's row less than a press can reach. Each creature must keep a strip the phone's
/// touch target tall and a cell of the hit test's grid more at each edge, between the item and
/// its far edge, so that the rows can stay where they are while a spell is on the stack.
fn stack_height(camera: &Camera, middle: f32) -> f32 {
    let uncovered = audit::PHONE_TARGET_POINTS + 2.0 * CELL;
    // A tapped card lies across its row, so its height on the table is a card's width.
    let half = CARD_WIDTH / 2.0;
    let reach = [data::creatures::Y, -data::creatures::Y]
        .map(|row| {
            let far = [row - half, row + half]
                .map(|y| (camera.to_screen(Vec3::new(0.0, y, 0.0)).y - middle).abs())
                .into_iter()
                .fold(0.0, f32::max);
            far - uncovered
        })
        .into_iter()
        .fold(f32::INFINITY, f32::min);
    (data::stack::WIDTH / RATIO).min(2.0 * reach)
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

/// The highest the top edge of the viewer's hand may reach on a phone, in logical points from the
/// window's top: low enough that each card of the viewer's back row, tapped, keeps a strip the
/// phone's touch target tall above it, and a cell of the hit test's grid more at each edge. A
/// hand of several rows that rose higher would cover the lands and other permanents, and then
/// the creatures, past what a press can reach; the cards that do not fit under this ceiling
/// scroll instead (`hands::Fan::shown`).
fn hand_ceiling(camera: &Camera) -> f32 {
    // A tapped card lies across its row, so its far edge is half a card's width from the row's
    // middle, toward the opponent.
    let far = camera
        .to_screen(Vec3::new(0.0, data::lands::Y + CARD_WIDTH / 2.0, 0.0))
        .y;
    far + audit::PHONE_TARGET_POINTS + 2.0 * CELL
}

/// The viewer's hand: cards a quarter of the column wide, at most 100 points, 4 points apart
/// while they have room, their top edges 8 points under the hand band's top (`placeHand`), and
/// in rows no higher than `ceiling` points from the window's top (`hand_ceiling`).
fn your_hand(window: Window, bands: &PhoneBands, ceiling: f32) -> HandData {
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
        ceiling: ndc(window, 0.0, ceiling).y,
    }
}

/// The opponent's hand: card backs along the top of their strip, at its right, short of the menu
/// (`backSpot`). The web's backs peek 10 points over the strip's top, into the top safe inset,
/// where a card the viewer may see, such as one revealed from that hand, would stand under the
/// notch and the status bar, so here they stand 2 points under the strip's top
/// (`opponent_hand.rise` is -2), which keeps the raised corners of the fan's tilted cards
/// inside the safe area too.
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

/// The space the cards a prompt holds up lie in on a phone `window`, in logical points (a stage
/// as tall as the window), as the web's `phoneTray` gives it: across the column, over the
/// opponent's rows, and the second of two piles over the viewer's own, clear of the row of
/// buttons the prompt stands over the viewer's strip (`prompts::bar`), so that the middle band
/// and the phase in it stay clear. Its
/// cards are at most as wide as a card of the hand, and keep `least_step` apart, as the stack's
/// items do, so that each card beneath keeps a square of the touch target uncovered.
#[must_use]
pub fn tray(window: Window) -> TraySpace {
    tray_in(window, &phone_bands(window).unwrap_or_default())
}

fn tray_in(window: Window, bands: &PhoneBands) -> TraySpace {
    use crate::data::space::hud::phone as hud;
    use data::tray as shape;
    let top = bands.opponent_back.top + shape::INSET;
    let foot = bands.middle.bottom() + shape::INSET;
    // The prompt's buttons stand on the dock's line while it holds cards up (`prompts::bar`):
    // the strip's middle, or over the top of the hand where it reaches higher. A hand with more
    // cards than one fan holds stands its back row in the strip, its top `HAND_TOP` over the
    // strip's top edge.
    let (button, gap) = (hud::BUTTON_HEIGHT, hud::DOCK_GAP);
    let hand_top = bands.your_strip.top - data::bands::HAND_TOP;
    let dock = bands.your_strip.middle().min(hand_top - gap - button / 2.0);
    let buttons = dock - button / 2.0 - gap;
    TraySpace {
        stage_height: window.size.height,
        y: top,
        height: bands.middle.top - shape::INSET - top,
        across: Across::Column {
            left: bands.left,
            width: bands.right - bands.left,
        },
        second: Some(Second {
            y: foot,
            height: buttons - foot,
        }),
        most: shape::MOST,
        least: shape::LEAST,
        gap: shape::GAP,
        least_step: shape::LEAST_STEP,
        heading: shape::HEADING,
        depth: DESKTOP.tray.depth,
    }
}

/// The viewer's life total's anchor on a phone `window` whose bands are `bands`, while the top
/// edge of their hand at rest is `hand_top` points from the window's top: the middle of their
/// strip, or higher, the dock's gap over the hand, where the hand reaches higher. A hand with
/// more cards than one fan holds stands its back row in the strip, and a total in the strip's
/// middle would stand on that row's uncovered part and split it into pieces too narrow to
/// press. The prompt's dock rises over the hand the same way (`prompts::bar`). A hand of three
/// rows or more reaches over the viewer's lands, and a total over it would stand on their
/// creatures, so the total rises only as far as it stays clear of the creatures' row.
#[must_use]
pub fn your_life(window: Window, bands: &PhoneBands, hand_top: f32) -> PlayerAnchor {
    use crate::data::space::hud::phone::DOCK_GAP;
    let half = super::HALF_LIFE;
    let middle = bands.your_strip.middle();
    let raised = hand_top - DOCK_GAP - half;
    let lowest_clear = bands.your_back.top + half;
    life_anchor(window, middle.min(raised.max(lowest_clear)))
}

/// A life total's anchor on a phone `window`, across the window's middle at `y` points from its
/// top. A phone shows no unspent mana, so a streak out of a pool starts at the
/// life total (`anchors` in `ui/src/stage/phone.ts`).
fn life_anchor(window: Window, y: f32) -> PlayerAnchor {
    let life = ndc(window, window.size.width / 2.0, y);
    PlayerAnchor { life, pool: life }
}

/// The phone layout for `window`, whose hands, piles, stack and anchors are converted from the
/// web interface's places in points to this window's normalized device coordinates.
#[must_use]
pub fn layout(window: Window) -> Layout {
    let camera_data = camera_data();
    let camera = Camera::fit_inside(camera_data, window, battlefield(window));
    let bands = bands(&camera);
    let (piles, pile_scale) = piles(&camera, &bands);
    // Each life total in the middle of its player's strip, until the table raises the viewer's
    // over a hand of several rows (`your_life`).
    let anchor = |strip: Band| life_anchor(window, strip.middle());
    let stack = PhoneStack {
        middle: bands.middle.middle(),
        height: stack_height(&camera, bands.middle.middle()),
        step: data::stack::STEP,
        left: bands.left,
        right: bands.right,
        count_room: data::stack::COUNT_ROOM,
        depth: data::stack::DEPTH,
        step_depth: data::stack::STEP_DEPTH,
    };
    let row = |y: f32, left: f32, right: f32, centred: bool| RowArea {
        y,
        left,
        right,
        centre: centred.then_some(f32::midpoint(left, right)),
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
        tray: tray_in(window, &bands),
        stack: DESKTOP.stack,
        hand: your_hand(window, &bands, hand_ceiling(&camera)),
        opponent_hand: opponent_hand(window, &bands),
        players: [anchor(bands.your_strip), anchor(bands.opponent_strip)],
        phone: Some(stack),
        bands: Some(bands),
        fitted: None,
        fitted_piles: None,
    }
}

/// The top edge on the window, in logical points, of the viewer's hand at the resting places
/// `rests` give (each a place, a pose and whether it is visible), before any card is held up in
/// a tray; infinity for an empty hand. Allocates nothing.
pub(super) fn hand_top(camera: &Camera, rests: impl Iterator<Item = (Place, Pose, bool)>) -> f32 {
    let mut top = f32::INFINITY;
    for (place, pose, visible) in rests {
        if visible
            && let Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            } = place
        {
            let corners = camera.card_box(&pose).0;
            top = corners.iter().fold(top, |top, corner| top.min(corner.y));
        }
    }
    top
}
