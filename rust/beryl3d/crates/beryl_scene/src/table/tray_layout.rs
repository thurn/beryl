//! Where the cards a prompt holds up lie (docs/beryl-3d.md, "Objects and placement"): a port of
//! the web interface's `laidOut` (`ui/src/stage/tray.ts`). One pile is laid out in rows about the
//! middle of the space, as many as it takes. Two are boxes side by side, each a row as long as
//! the tray holds cards, so that a box keeps its place whatever is moved between them. Two kinds
//! of split are laid out apart from that, as the web lays them out:
//!
//! - Cards picked out of the library from among the rest (`TrayKind::Fan`, the web's `picks`)
//!   lie in one overlapping fan in the order they were dealt, as Arena's "Choose Two" browser
//!   has them, with no boxes; the prompt's marks say which are picked.
//! - Cards put back from the hand onto the library (`TrayKind::PutBack`) lie as the hand and the
//!   library do: the library face down at the left with the cards put on it fanned down its
//!   face, and the hand fanned at the right. Each pile still has a box where a card let go lands
//!   in it, drawn with no edges.
//!
//! The web lays a tray out on its 1600 by 900 stage, in the space `desktopTray`
//! (`ui/src/stage/desktop.ts`) gives. Here the same space is measured in stage pixels of a stage
//! as tall as the window, `layout.desktop.tray` in `space.json`, and each card is placed relative
//! to the camera, so that the tray keeps its size and place on screen at any window shape.

use beryl_protocol::protocol::ObjectId;
use glam::{Quat, Vec2};

use super::{CARD_HEIGHT, CARD_WIDTH, Camera};
use crate::data::space::layout::desktop::tray as data;
use crate::input::{Rect, Size};
use crate::pose::Pose;

/// The space a layout gives a tray and the sizes it lays one out at, in stage pixels of a stage
/// `stage_height` tall and as wide as the window's shape makes it, as the web's `TraySpace`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraySpace {
    pub stage_height: f32,
    /// The top of the space, and how tall it is.
    pub y: f32,
    pub height: f32,
    /// The most the space reaches either side of the middle, and how far from the stage's right
    /// edge its own right edge must stay, clear of the stack: `desktopTray`'s
    /// `middle - fromMiddle(150)` and `STACK.x - 16 - middle`.
    pub half_most: f32,
    pub stack_side: f32,
    /// The widest and the narrowest a card is drawn.
    pub most: f32,
    pub least: f32,
    /// Between two cards, and between a box's edge and its cards.
    pub gap: f32,
    /// The strip over a box's cards that says what the pile is.
    pub heading: f32,
    /// How far in front of the eye the first card lies, in millimetres; each later card lies a
    /// millimetre nearer, so that it covers the one before it.
    pub depth: f32,
}

impl TraySpace {
    /// The desktop layout's, from `layout.desktop.tray`.
    pub const DESKTOP: Self = Self {
        stage_height: data::STAGE_HEIGHT,
        y: data::Y,
        height: data::HEIGHT,
        half_most: data::HALF_MOST,
        stack_side: data::STACK_SIDE,
        most: data::MOST,
        least: data::LEAST,
        gap: data::GAP,
        heading: data::HEADING,
        depth: data::DEPTH,
    };

    /// The space on a window of `size`, in logical points of the window, and how many logical
    /// points a stage pixel is.
    #[must_use]
    pub fn on_window(&self, size: Size) -> (Rect, f32) {
        let per_point = self.stage_height / size.height;
        let (left, wide) = across(self, size.width * per_point);
        let rect = Rect {
            x: left / per_point,
            y: self.y / per_point,
            width: wide / per_point,
            height: self.height / per_point,
        };
        (rect, 1.0 / per_point)
    }
}

/// How a tray of two piles is laid out, as the web's `Tray.picks` and `Tray.putBack` say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TrayKind {
    /// Each pile in a box of its own, side by side.
    #[default]
    Boxes,
    /// The first pile's cards picked out of the second's, which stay in the library: every card
    /// in one fan, in `order`, the order they were dealt in, kept from the tray up before so
    /// that a card clicked stays where it lies.
    Fan { order: Vec<ObjectId> },
    /// The second pile's cards put back from the hand onto the library, the first pile.
    PutBack,
}

/// The cards a prompt holds up: one or two piles, each its cards' slots in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub piles: Vec<Vec<ObjectId>>,
    /// How many cards each pile of two is laid out for, where the prompt says it must come to
    /// hold so many, as the web's `Heap.room`. A pile with none here is laid out for every card
    /// the tray holds, so that its box keeps its place whatever is moved between the piles.
    pub rooms: Vec<Option<usize>>,
    pub kind: TrayKind,
}

/// Where the cards of a tray lie, seen through the camera: one pose for each card, by slot, in
/// the order of the piles and then of their cards; and, for a put-back, each card back the
/// library is drawn with, the deepest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrayPoses {
    pub cards: Vec<(ObjectId, Pose)>,
    pub library: Vec<Pose>,
}

/// A tray laid out on the stage, as the web's `Laid`: where each card lies, in the order of the
/// piles and then of their cards; the box of each pile of a tray of two, where a card let go
/// lands in it; and for a put-back, where the library is drawn and the top edge of each place
/// in its column, the deepest first, as many as the library's pile can come to hold.
struct Laid {
    spots: Vec<Spot>,
    boxes: Vec<Area>,
    library: Option<Area>,
    column: Vec<f32>,
}

impl Laid {
    fn of(spots: Vec<Spot>, boxes: Vec<Area>) -> Self {
        Self {
            spots,
            boxes,
            library: None,
            column: Vec::new(),
        }
    }
}

/// A rectangle of the stage, in stage pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Area {
    fn contains(&self, point: Vec2) -> bool {
        (self.x..=self.x + self.width).contains(&point.x)
            && (self.y..=self.y + self.height).contains(&point.y)
    }
}

/// A card's place on the stage: its top left corner, its width, how far it is turned clockwise
/// in degrees, and where it comes in the order the cards are drawn, the deepest first.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    x: f32,
    y: f32,
    width: f32,
    turn: f32,
    drawn: usize,
}

impl Spot {
    fn flat(x: f32, y: f32, width: f32, drawn: usize) -> Self {
        Self {
            x,
            y,
            width,
            turn: 0.0,
            drawn,
        }
    }
}

/// The space's left edge and width on a stage `stage_width` wide.
fn across(space: &TraySpace, stage_width: f32) -> (f32, f32) {
    let middle = stage_width / 2.0;
    let half = space.half_most.min(middle - space.stack_side).max(0.0);
    (middle - half, 2.0 * half)
}

/// The widths a card is tried at, the widest first: from `most` down by 8 pixels, and `least`.
fn widths(space: &TraySpace) -> impl Iterator<Item = f32> + '_ {
    let mut width = space.most;
    std::iter::from_fn(move || {
        if width > space.least {
            let tried = width;
            width -= 8.0;
            Some(tried)
        } else if width > f32::MIN {
            width = f32::MIN;
            Some(space.least)
        } else {
            None
        }
    })
}

/// How tall a card `width` wide is.
fn height_of(width: f32) -> f32 {
    width * CARD_HEIGHT / CARD_WIDTH
}

/// Where the `index`th of `count` cards lies along a fan, from -1 at its left end to 1 at its
/// right; 0 for a fan of one.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn along(index: usize, count: usize) -> f32 {
    if count > 1 {
        index as f32 / (count - 1) as f32 * 2.0 - 1.0
    } else {
        0.0
    }
}

/// One pile of `count` cards in rows about the middle of the space, as the web's `rows`.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a tray holds a few dozen cards"
)]
fn rows(count: usize, space: &TraySpace, (left, wide): (f32, f32)) -> Vec<Spot> {
    let gap = space.gap;
    let total = count.max(1);
    let (mut width, mut across, mut step) = (space.least, total, 0.0);
    for tried in widths(space) {
        let fits = (((wide + gap) / (tried + gap)).floor() as usize).max(1);
        let deep = total.div_ceil(fits);
        width = tried;
        across = total.min(fits);
        step = tried + gap;
        if deep as f32 * (height_of(tried) + gap) - gap <= space.height {
            break;
        }
        if tried <= space.least {
            // Too many for whole cards: the rows the space holds, each closed up over itself.
            let held = (((space.height + gap) / (height_of(tried) + gap)).floor() as usize).max(1);
            across = total.div_ceil(held);
            step = f32::min(step, (wide - tried) / (across.max(2) - 1) as f32);
        }
    }
    let deep = total.div_ceil(across);
    let tall = deep as f32 * (height_of(width) + gap) - gap;
    let top = space.y + ((space.height - tall) / 2.0).max(0.0);
    (0..count)
        .map(|index| {
            let row = index / across;
            let in_row = across.min(count - row * across);
            let row_wide = width + (in_row as f32 - 1.0) * step;
            Spot::flat(
                left + (wide - row_wide) / 2.0 + (index % across) as f32 * step,
                top + row as f32 * (height_of(width) + gap),
                width,
                index,
            )
        })
        .collect()
}

/// Several piles, a box each, side by side about the middle of the space, as the web's
/// `boxes`. Each box has room for what its pile can come to hold (`rooms`, the web's
/// `roomsOf`), and its cards lie in a row from its left edge, closed up where they would
/// overflow it.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn boxes(counts: &[usize], rooms: &[usize], space: &TraySpace, (left, wide): (f32, f32)) -> Laid {
    let (gap, heading) = (space.gap, space.heading);
    let piles = counts.len() as f32;
    let between: f32 = rooms.iter().map(|&room| room.max(1) as f32 - 1.0).sum();
    // What is not a card's own width or the step to the next: the boxes' edges and the gaps
    // between the boxes.
    let edges = piles * 2.0 * gap + (piles - 1.0) * gap;
    let (mut width, mut step) = (space.least, 0.0);
    for tried in widths(space) {
        width = tried;
        step = tried + gap;
        let whole = piles * tried + between * step + edges;
        let tall = height_of(tried) + heading + 2.0 * gap;
        if whole <= wide && tall <= space.height {
            break;
        }
        if tried <= space.least && between > 0.0 {
            step = f32::min(step, (wide - edges - piles * tried) / between);
        }
    }
    let height = height_of(width) + heading + 2.0 * gap;
    let whole = piles * width + between * step + edges;
    let y = space.y + ((space.height - height) / 2.0).max(0.0);
    let mut x = left + (wide - whole) / 2.0;
    let mut laid = Laid::of(
        Vec::with_capacity(counts.iter().sum()),
        Vec::with_capacity(counts.len()),
    );
    for (&count, &room) in counts.iter().zip(rooms) {
        let box_wide = width + (room.max(1) as f32 - 1.0) * step + 2.0 * gap;
        let own = f32::min(
            step,
            (box_wide - 2.0 * gap - width) / (count.max(2) - 1) as f32,
        );
        for index in 0..count {
            let drawn = laid.spots.len();
            laid.spots.push(Spot::flat(
                x + gap + index as f32 * own,
                y + heading + gap,
                width,
                drawn,
            ));
        }
        laid.boxes.push(Area {
            x,
            y,
            width: box_wide,
            height,
        });
        x += box_wide + gap;
    }
    laid
}

/// The cards of a pick, `order` in its order, in one fan about the middle of the space, as
/// large as its height holds and its width takes, as the web's `fan`; each card's spot in the
/// order of `piles`.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn fan(
    piles: &[Vec<ObjectId>],
    order: &[ObjectId],
    space: &TraySpace,
    (left, wide): (f32, f32),
) -> Laid {
    let gap = space.gap;
    let aspect = CARD_WIDTH / CARD_HEIGHT;
    let count = order.len().max(1);
    let across = wide - 2.0 * gap;
    let tall = (space.height - 2.0 * gap) * aspect;
    let width = f32::max(
        space.least,
        f32::min(
            tall / (1.0 + data::fan::DROP * aspect),
            across / (1.0 + data::fan::STEP * (count - 1) as f32),
        ),
    );
    let step = f32::min(
        data::fan::STEP * width,
        (across - width) / (count.max(2) - 1) as f32,
    );
    let whole = width + step * (count - 1) as f32;
    let start = left + (wide - whole) / 2.0;
    let top = space.y + (space.height - height_of(width) - data::fan::DROP * width) / 2.0;
    let spot = |index: usize| {
        let t = along(index, count);
        Spot {
            x: start + step * index as f32,
            y: top + data::fan::DROP * width * t * t,
            width,
            turn: data::fan::TURN * t,
            drawn: index,
        }
    };
    let spots = (piles.iter().flatten())
        .map(|card| spot(order.iter().position(|dealt| dealt == card).unwrap_or(0)))
        .collect();
    Laid::of(spots, Vec::new())
}

/// The room each pile of `held` is laid out for: what it must come to hold, or all the tray
/// holds, as the web's `roomsOf`.
fn rooms_of(held: &Held) -> Vec<usize> {
    let total = held.piles.iter().map(Vec::len).sum();
    (0..held.piles.len())
        .map(|pile| {
            held.rooms
                .get(pile)
                .copied()
                .flatten()
                .unwrap_or(total)
                .max(1)
        })
        .collect()
}

/// Cards put back from the hand (the second pile) onto the library (the first), each pile laid
/// out for what it can come to hold, as the web's `putBack`: the library at the left of the
/// space, the cards put on it fanned down its face, the top one lowest and in front, and the
/// hand fanned at the right. The two piles' boxes share one strip and part half way between the
/// library and the fan, so that a card let go left of there goes on the library.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn put_back(held: &Held, space: &TraySpace, (left, wide): (f32, f32)) -> Laid {
    use data::put_back as shape;
    let gap = space.gap;
    let (onto, hand) = (&held.piles[0], &held.piles[1]);
    let rooms = rooms_of(held);
    let (deep, room) = (rooms[0] as f32, rooms[1] as f32);
    // The widest card for which the library's column fits the height, and the whole fits across
    // with the fan's neighbors no closer than `least_step` of a card.
    let tall = |width: f32| height_of(width) * (1.0 + shape::CASCADE * (deep - 1.0));
    let (mut width, mut step) = (space.least, 0.0);
    for tried in widths(space) {
        width = tried;
        let across = wide - 2.0 * gap - tried * (1.0 + shape::BETWEEN);
        step = f32::min(
            shape::STEP * tried,
            (across - tried) / (room - 1.0).max(1.0),
        );
        if tall(tried) <= space.height - 2.0 * gap && step >= shape::LEAST_STEP * tried {
            break;
        }
    }
    let height = height_of(width);
    let fanned = width + step * (room - 1.0).max(0.0);
    let whole = width * (1.0 + shape::BETWEEN) + fanned;
    let library_x = left + gap.max((wide - whole) / 2.0);
    let span = f32::min(
        shape::CASCADE * height,
        (space.height - 2.0 * gap - height) / (deep - 1.0).max(1.0),
    );
    let column = height + span * (deep - 1.0);
    let top = space.y + (space.height - column) / 2.0;
    let library = Area {
        x: library_x,
        y: top - shape::LIBRARY_RISE * height,
        width,
        height,
    };
    // The hand's fan, as long as the cards in it now, about the middle of the room it has. The
    // web draws the hand's cards first, then the library's column.
    let hand_x = library_x + width * (1.0 + shape::BETWEEN);
    let count = hand.len();
    let start = hand_x + (fanned - (width + step * (count as f32 - 1.0))) / 2.0;
    let middle = space.y + (space.height - height) / 2.0;
    let mut spots = Vec::with_capacity(onto.len() + count);
    // The library's column, the deepest first, while its pile lists the top first.
    for index in 0..onto.len() {
        let down = onto.len() - 1 - index;
        let [even, odd] = shape::COLUMN_TURNS.to_array();
        spots.push(Spot {
            x: library_x,
            y: top + span * down as f32,
            width,
            // The top card, in front, lies straight; those under it lean one way and the other.
            turn: if index == 0 {
                0.0
            } else if down % 2 == 0 {
                even
            } else {
                odd
            },
            drawn: count + down,
        });
    }
    for index in 0..count {
        let t = along(index, count);
        spots.push(Spot {
            x: start + step * index as f32,
            y: middle + shape::DROP * width * t * t,
            width,
            turn: shape::TURN * t,
            drawn: index,
        });
    }
    let strip_top = f32::min(library.y - space.heading, middle - space.heading - gap);
    let strip_bottom = f32::max(
        top + column + gap,
        middle + height + shape::DROP * width + gap,
    );
    let reach = (library_x + width + hand_x) / 2.0;
    let strip = |x: f32, width: f32| Area {
        x,
        y: strip_top,
        width,
        height: strip_bottom - strip_top,
    };
    Laid {
        spots,
        boxes: vec![strip(left, reach - left), strip(reach, left + wide - reach)],
        library: Some(library),
        column: (0..rooms[0])
            .map(|place| top + span * place as f32)
            .collect(),
    }
}

/// Piles of `counts` cards laid out in `span`: one pile in rows, several in boxes with room for
/// `rooms` (every card of the tray for a pile with none).
fn laid_piles(
    counts: &[usize],
    rooms: &[Option<usize>],
    space: &TraySpace,
    span: (f32, f32),
) -> Laid {
    if counts.len() == 1 {
        return Laid::of(rows(counts[0], space, span), Vec::new());
    }
    let total = counts.iter().sum();
    let rooms: Vec<usize> = (0..counts.len())
        .map(|pile| rooms.get(pile).copied().flatten().unwrap_or(total))
        .collect();
    boxes(counts, &rooms, space, span)
}

/// `held` laid out in `space` on a stage `stage_width` wide, as the web's `laidOut`.
fn laid_out(held: &Held, space: &TraySpace, stage_width: f32) -> Laid {
    let span = across(space, stage_width);
    match &held.kind {
        TrayKind::PutBack if held.piles.len() == 2 => put_back(held, space, span),
        TrayKind::Fan { order } => fan(&held.piles, order, space, span),
        _ => {
            let counts: Vec<usize> = held.piles.iter().map(Vec::len).collect();
            laid_piles(&counts, &held.rooms, space, span)
        }
    }
}

/// Stage pixels per logical point of a window `size`, and the width of the stage it shows.
fn stage_of(space: &TraySpace, size: Size) -> (f32, f32) {
    let per_point = space.stage_height / size.height;
    (per_point, size.width * per_point)
}

/// The part of a window `size` that `space` gives a prompt's tray, in logical points: the
/// space's band of the stage, as wide as it may reach about the middle, as the web's
/// `desktopTray` gives it.
#[must_use]
pub fn tray_area(space: TraySpace, size: Size) -> Rect {
    let (per_point, stage_width) = stage_of(&space, size);
    let (left, wide) = across(&space, stage_width);
    Rect {
        x: left / per_point,
        y: space.y / per_point,
        width: wide / per_point,
        height: space.height / per_point,
    }
}

/// The box of each pile of `held`, laid out in `space`, on a window `size`, in logical points,
/// as the web's `TrayUnder` draws them: none for a tray of one pile or a fan.
#[must_use]
pub fn tray_boxes(held: &Held, space: TraySpace, size: Size) -> Vec<Rect> {
    let (per_point, stage_width) = stage_of(&space, size);
    let laid = laid_out(held, &space, stage_width);
    (laid.boxes.iter())
        .map(|area| Rect {
            x: area.x / per_point,
            y: area.y / per_point,
            width: area.width / per_point,
            height: area.height / per_point,
        })
        .collect()
}

/// Where the card `carried` let go at `point`, in logical points of a window `size`, lands in
/// `held` laid out in `space`, as the web's `landing`: the index of the pile whose box the point
/// is in, and before which of that pile's cards, counting those whose middle lies left of the
/// point (the pile's length for its end). On the library of a put-back it lands in the place of
/// the library's column nearest to the point (`library_at`). None outside every box, and in a
/// tray of one pile or a fan, which have no order to give.
#[must_use]
pub fn landing(
    held: &Held,
    space: TraySpace,
    size: Size,
    point: Vec2,
    carried: ObjectId,
) -> Option<(usize, usize)> {
    let (per_point, stage_width) = stage_of(&space, size);
    let point = point * per_point;
    let laid = laid_out(held, &space, stage_width);
    let pile = laid.boxes.iter().position(|area| area.contains(point))?;
    if let (0, Some(library)) = (pile, laid.library) {
        return Some((
            0,
            library_at(&held.piles[0], &laid.column, library, point, carried),
        ));
    }
    let first: usize = held.piles[..pile].iter().map(Vec::len).sum();
    let spots = &laid.spots[first..first + held.piles[pile].len()];
    Some((
        pile,
        (spots.iter())
            .filter(|spot| spot.x + spot.width / 2.0 < point.x)
            .count(),
    ))
}

/// Where on the library of a put-back the card `carried`, let go with its middle at `point`,
/// lands, as the web's `library`: the place of the library's `column` (`cards` lie there now,
/// the top first) that, once the card is in it, lies nearest to where its middle was let go. A
/// held card is drawn with its middle under the pointer (`pointer::held::pose`), so the point is
/// its middle. Comparing the point with the middles of the cards, as a row's landing does, would
/// leave each place a strip only a name bar tall.
fn library_at(
    cards: &[ObjectId],
    column: &[f32],
    library: Area,
    point: Vec2,
    carried: ObjectId,
) -> usize {
    let own = cards.iter().position(|&card| card == carried);
    let others = cards.len() - usize::from(own.is_some());
    // The places it could take, the deepest first: one more than the others hold.
    let places = &column[..column.len().min(others + 1)];
    let off = |top: f32| (top + library.height / 2.0 - point.y).abs();
    let nearest = (0..places.len())
        .min_by(|&a, &b| off(places[a]).total_cmp(&off(places[b])))
        .unwrap_or(0);
    // Its index among the others, the top first; below where it lies now, one more, as the
    // drop's `at` counts the card itself in its place (the session takes it out before putting
    // it back).
    let among = others - nearest;
    match own {
        Some(own) if among > own => among + 1,
        _ => among,
    }
}

/// The pose, seen through `camera`, of a card at `area`'s top left corner and `area.width`
/// wide, turned `turn` degrees clockwise, `depth` millimetres in front of the eye.
fn screen_pose(
    camera: &Camera,
    space: &TraySpace,
    stage_width: f32,
    (x, y, width): (f32, f32, f32),
    turn: f32,
    depth: f32,
) -> Pose {
    let height = height_of(width);
    let centre = Vec2::new(x + width / 2.0, y + height / 2.0);
    let ndc = Vec2::new(
        centre.x / stage_width * 2.0 - 1.0,
        1.0 - centre.y / space.stage_height * 2.0,
    );
    // Turning about the camera's z, which points at the viewer, turns a card counterclockwise.
    let rotation = Quat::from_rotation_z(-turn.to_radians());
    camera.screen_pose(ndc, depth, rotation, height / space.stage_height)
}

/// Where each card of `laid` lies, seen through `camera`; a card drawn later lies nearer the eye.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn poses(laid: &Laid, space: &TraySpace, stage_width: f32, camera: &Camera) -> Vec<Pose> {
    (laid.spots.iter())
        .map(|spot| {
            let depth = space.depth - spot.drawn as f32;
            let at = (spot.x, spot.y, spot.width);
            screen_pose(camera, space, stage_width, at, spot.turn, depth)
        })
        .collect()
}

/// Where each card of `piles`, one pile or two, each its cards in order, lies held up in
/// `space`, seen through `camera`: one pose for each card, in the order of the piles and then
/// of their cards. Each is face up and faces the camera; a later card lies nearer the eye.
/// Each of two piles is laid out in a box with room for the cards it holds.
#[must_use]
pub fn tray<T>(piles: &[&[T]], space: TraySpace, camera: &Camera) -> Vec<Pose> {
    let (_, stage_width) = stage_of(&space, camera.window.size);
    let counts: Vec<usize> = piles.iter().map(|pile| pile.len()).collect();
    let rooms: Vec<Option<usize>> = counts.iter().copied().map(Some).collect();
    let span = across(&space, stage_width);
    let laid = laid_piles(&counts, &rooms, &space, span);
    poses(&laid, &space, stage_width, camera)
}

/// Where each card of `held` lies held up in `space`, seen through `camera`, and each card back
/// a put-back's library is drawn with: as many as `deck_backs`, each `deck_offset` stage pixels
/// from the one over it, so that the library's edge shows as a deck's does (the web's `Library`
/// in `ui/src/stage/Tray.tsx`), all behind the cards of the tray.
#[must_use]
pub fn tray_poses(held: &Held, space: TraySpace, camera: &Camera) -> TrayPoses {
    let (_, stage_width) = stage_of(&space, camera.window.size);
    let laid = laid_out(held, &space, stage_width);
    let cards = (held.piles.iter().flatten().copied())
        .zip(poses(&laid, &space, stage_width, camera))
        .collect();
    let backs = data::put_back::DECK_BACKS;
    let library = (laid.library.into_iter())
        .flat_map(|area| {
            (0..backs).map(move |index| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a deck is drawn with a few backs"
                )]
                let under = (backs - 1 - index) as f32;
                let offset = data::put_back::DECK_OFFSET * under;
                let at = (area.x + offset.x, area.y + offset.y, area.width);
                screen_pose(
                    camera,
                    &space,
                    stage_width,
                    at,
                    0.0,
                    space.depth + 1.0 + under,
                )
            })
        })
        .collect();
    TrayPoses { cards, library }
}
