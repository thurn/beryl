//! Where the cards a prompt holds up lie (docs/beryl-3d.md, "Objects and placement"): a port of
//! the web interface's `laidOut` (`ui/src/stage/tray.ts`). One pile is laid out in rows about the
//! middle of the space, as many as it takes. Two are boxes side by side, each a row as long as
//! the tray holds cards, so that a box keeps its place whatever is moved between them, or on a
//! phone, which is too narrow for that, one box over the other (`TraySpace::second`). Two kinds
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
//! as tall as the window, with bounds from the layout profile. Each card is placed relative
//! to the camera, so that the tray keeps its size and place on screen at any window shape.
//!
//! The phone's space is the web's `phoneTray` (`ui/src/stage/phone.ts`), measured in logical
//! points (a stage as tall as the window) from the phone's bands (`Profile::tray`). Its
//! cards keep `TraySpace::least_step` apart wherever they overlap, so that each card beneath
//! keeps a square of the phone's touch target uncovered: a fan or a pile closes up no further
//! than that, and lies in rows that overlap where one row has no room.

use beryl_protocol::protocol::ObjectId;
use glam::{Quat, Vec2};
use std::ops::Range;

use super::Camera;
use kit_base::data::Card;
use kit_base::data::profiles::TrayTuning;
use kit_base::pose::Pose;
use kit_space::arrange::Grid;
use kit_ui::input::{Rect, Size};

mod put_back;

use put_back::{library_at, put_back};

/// The space a layout gives a tray and the sizes it lays one out at, in stage pixels of a stage
/// `stage_height` tall and as wide as the window's shape makes it, as the web's `TraySpace`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraySpace {
    pub stage_height: f32,
    /// The top of the space, and how tall it is.
    pub y: f32,
    pub height: f32,
    /// How far across the stage the space reaches.
    pub across: Across,
    /// Where the second of two piles lies, where the layout gives it a space of its own under
    /// the first (the web's `TraySpace.second`): a phone is too narrow for two boxes side by
    /// side. None on the desktop.
    pub second: Option<Second>,
    /// The widest and the narrowest a card is drawn.
    pub most: f32,
    pub least: f32,
    /// Between two cards, and between a box's edge and its cards.
    pub gap: f32,
    /// The least distance between the left edges of two cards that overlap, and between the top
    /// edges of two rows that overlap, so that each card beneath keeps a square this size at its
    /// top left uncovered, where a press reaches it. Zero on the desktop, whose trays have room
    /// for cards that overlap less than a square of the touch target.
    pub least_step: f32,
    /// The minimum press square of a flat grid, in logical points.
    pub grid_target: f32,
    /// The strip over a box's cards that says what the pile is.
    pub heading: f32,
    pub tuning: TrayTuning,
    /// The card the tray holds up, whose shape sets how tall a card of a given width is.
    pub card: Card,
}

impl TraySpace {
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

/// How far across the stage a tray's space reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Across {
    /// About the stage's middle, as the web's `desktopTray` gives it: at most `half_most` either
    /// side of the middle, and its right edge at least `stack_side` from the stage's right edge,
    /// clear of the stack (`middle - fromMiddle(150)` and `STACK.x - 16 - middle`).
    Middle { half_most: f32, stack_side: f32 },
    /// Between fixed edges of the stage, as the web's `phoneTray` gives it: the phone's column.
    Column { left: f32, width: f32 },
}

/// The band of the stage the second of two piles lies in, across the same width as the first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Second {
    pub y: f32,
    pub height: f32,
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
    pub(super) grids: Vec<TrayGrid>,
}

/// A grid's viewport and its cards in `TrayPoses::cards`, in logical points.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TrayGrid {
    pub cards: Range<usize>,
    pub region: Rect,
    pub limit: f32,
    pub offset: f32,
    pub name: String,
    pub labels: Vec<String>,
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
    grids: Vec<TrayGrid>,
}

impl Laid {
    fn of(spots: Vec<Spot>, boxes: Vec<Area>) -> Self {
        Self {
            spots,
            boxes,
            library: None,
            column: Vec::new(),
            grids: Vec::new(),
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
    match space.across {
        Across::Middle {
            half_most,
            stack_side,
        } => {
            let middle = stage_width / 2.0;
            let half = half_most.min(middle - stack_side).max(0.0);
            (middle - half, 2.0 * half)
        }
        Across::Column { left, width } => (left, width),
    }
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

/// How tall a card `width` wide is, in a tray of `space`.
fn height_of(space: &TraySpace, width: f32) -> f32 {
    width * space.card.height / space.card.width
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

/// One pile of cards centred in the tray band.
fn rows(count: usize, space: &TraySpace, (left, wide): (f32, f32)) -> Laid {
    let grid = Grid::fit(
        count,
        Vec2::new(wide, space.height),
        space.card.height / space.card.width,
        space.gap,
        space.grid_target.max(space.least_step),
        widths(space),
    );
    let mut laid = Laid::of(Vec::with_capacity(count), Vec::new());
    laid.grid(
        grid,
        count,
        Rect {
            x: left,
            y: space.y,
            width: wide,
            height: space.height,
        },
        true,
    );
    laid
}

impl Laid {
    /// Adds one grid and records the viewport that clips it when its rows overflow.
    fn grid(&mut self, grid: Grid, count: usize, region: Rect, centred: bool) {
        let start = self.spots.len();
        self.spots.extend(
            grid.boxes(count, region, centred)
                .enumerate()
                .map(|(index, rect)| Spot::flat(rect.x, rect.y, rect.width, start + index)),
        );
        self.grids.push(TrayGrid {
            cards: start..self.spots.len(),
            region,
            limit: (grid.size(count).y - region.height).max(0.0),
            offset: 0.0,
            name: String::new(),
            labels: Vec::new(),
        });
    }

    /// The surrounding drop area keeps the room it reserved for this pile even as
    /// cards move between the piles.
    fn pile_grid(
        &mut self,
        count: usize,
        space: &TraySpace,
        region: Rect,
        (width, pitch): (f32, f32),
    ) {
        let grid = Grid::reserved_row(
            count,
            region.width,
            Vec2::new(width, height_of(space, width)),
            pitch,
            space.gap,
            space.grid_target,
        );
        self.grid(grid, count, region, false);
    }
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
        let tall = height_of(space, tried) + heading + 2.0 * gap;
        if whole <= wide && tall <= space.height {
            break;
        }
        if tried <= space.least && between > 0.0 {
            step = f32::min(step, (wide - edges - piles * tried) / between);
        }
    }
    let height = height_of(space, width) + heading + 2.0 * gap;
    let whole = piles * width + between * step + edges;
    let y = space.y + ((space.height - height) / 2.0).max(0.0);
    let mut x = left + (wide - whole) / 2.0;
    let mut laid = Laid::of(
        Vec::with_capacity(counts.iter().sum()),
        Vec::with_capacity(counts.len()),
    );
    for (&count, &room) in counts.iter().zip(rooms) {
        let box_wide = width + (room.max(1) as f32 - 1.0) * step + 2.0 * gap;
        let region = Rect {
            x: x + gap,
            y: y + heading + gap,
            width: box_wide - 2.0 * gap,
            height: height_of(space, width),
        };
        laid.pile_grid(count, space, region, (width, step));
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

/// Two piles, each a box as wide as the space, the first in its band and the second in
/// `second`, as the web's `stacked`: each box a row with room for what its pile can come to
/// hold (`rooms`), its cards no closer than `least_step` while the pile holds no more than
/// that, at one width for both, no larger than the shorter band holds.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn stacked(
    counts: &[usize],
    rooms: &[usize],
    space: &TraySpace,
    second: Second,
    (left, wide): (f32, f32),
) -> Laid {
    let (gap, heading) = (space.gap, space.heading);
    let inner = wide - 2.0 * gap;
    let most_room = rooms.iter().copied().max().unwrap_or(1).max(1);
    let tall = space.height.min(second.height) - heading - 2.0 * gap;
    // As wide as the room keeps apart, at least the least width, unless the shorter band holds
    // less: a narrower card is better than one under the prompt's buttons.
    let width = (inner - space.least_step * (most_room - 1) as f32)
        .min(space.most)
        .max(space.least)
        .min((tall * space.card.width / space.card.height).max(space.least_step));
    let height = height_of(space, width) + heading + 2.0 * gap;
    let bands = [(space.y, space.height), (second.y, second.height)];
    let mut laid = Laid::of(
        Vec::with_capacity(counts.iter().sum()),
        Vec::with_capacity(counts.len()),
    );
    for ((&count, &room), (band_y, band_height)) in counts.iter().zip(rooms).zip(bands) {
        let y = band_y + ((band_height - height) / 2.0).max(0.0);
        let step = Grid::pitch(inner, width, room.max(2), gap);
        let region = Rect {
            x: left + gap,
            y: y + heading + gap,
            width: inner,
            height: height_of(space, width),
        };
        laid.pile_grid(count, space, region, (width, step));
        laid.boxes.push(Area {
            x: left,
            y,
            width: wide,
            height,
        });
    }
    laid
}

/// The cards of a pick, `order` in its order, in one fan about the middle of the space, as
/// large as its height holds and its width takes, as the web's `fan`; each card's spot in the
/// order of `piles`. Where neighbours that keep `least_step` between them would leave no room
/// for a card of the least width, the cards lie in rows instead, in the same order.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn fan(
    piles: &[Vec<ObjectId>],
    order: &[ObjectId],
    space: &TraySpace,
    (left, wide): (f32, f32),
) -> Laid {
    let gap = space.gap;
    let aspect = space.card.width / space.card.height;
    let count = order.len().max(1);
    let across = wide - 2.0 * gap;
    let tall = (space.height - 2.0 * gap) * aspect;
    let between = space.least_step * (count - 1) as f32;
    let width = f32::max(
        space.least,
        f32::min(
            f32::min(
                tall / (1.0 + space.tuning.fan.drop * aspect),
                across / (1.0 + space.tuning.fan.step * (count - 1) as f32),
            ),
            across - between,
        ),
    );
    if space.least_step > 0.0 && width + between > across {
        let mut laid = rows(order.len(), space, (left, wide));
        laid.spots = in_order(piles, order, |index| laid.spots[index]);
        return laid;
    }
    let step = f32::min(
        space.tuning.fan.step * width,
        (across - width) / (count.max(2) - 1) as f32,
    )
    .max(space.least_step);
    let height = height_of(space, width);
    let drop = space.tuning.fan.drop * width;
    let top = space.y + (space.height - height - drop) / 2.0;
    let grid = Grid::new(
        count,
        Vec2::new(width, height),
        Vec2::new(step - width, 0.0),
    );
    let whole = grid.size(count).x;
    let region = Rect {
        x: left + (wide - whole) / 2.0,
        y: top,
        width: whole,
        height: height + drop,
    };
    let mut laid = Laid::of(Vec::with_capacity(order.len()), Vec::new());
    laid.grid(grid, order.len(), region, false);
    curve_fan(&mut laid.spots, drop, space.tuning.fan.turn);
    laid.spots = in_order(piles, order, |index| laid.spots[index]);
    laid
}

/// Curves a straight grid row into a fan without changing the order of its cards.
fn curve_fan(spots: &mut [Spot], drop: f32, turn: f32) {
    let count = spots.len();
    for (index, spot) in spots.iter_mut().enumerate() {
        let t = along(index, count);
        spot.y += drop * t * t;
        spot.turn = turn * t;
        spot.drawn = index;
    }
}

/// The spot of each card of `piles`, in their order, from `spot`, which gives the spot of the
/// card at each index of `order`.
fn in_order(
    piles: &[Vec<ObjectId>],
    order: &[ObjectId],
    spot: impl Fn(usize) -> Spot,
) -> Vec<Spot> {
    (piles.iter().flatten())
        .map(|card| spot(order.iter().position(|dealt| dealt == card).unwrap_or(0)))
        .collect()
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

/// Piles of `counts` cards laid out in `span`: one pile in rows, several in boxes with room for
/// `rooms` (every card of the tray for a pile with none).
fn laid_piles(
    counts: &[usize],
    rooms: &[Option<usize>],
    space: &TraySpace,
    span: (f32, f32),
) -> Laid {
    if counts.len() == 1 {
        return rows(counts[0], space, span);
    }
    let total = counts.iter().sum();
    let rooms: Vec<usize> = (0..counts.len())
        .map(|pile| rooms.get(pile).copied().flatten().unwrap_or(total))
        .collect();
    match space.second {
        Some(second) if counts.len() == 2 => stacked(counts, &rooms, space, second, span),
        _ => boxes(counts, &rooms, space, span),
    }
}

/// `held` laid out in `space` on a stage `stage_width` wide, as the web's `laidOut`.
fn laid_out(held: &Held, mut space: TraySpace, size: Size) -> Laid {
    let (per_point, stage_width) = stage_of(&space, size);
    space.grid_target *= per_point;
    let space = &space;
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
    space.on_window(size).0
}

/// The box of each pile of `held`, laid out in `space`, on a window `size`, in logical points,
/// as the web's `TrayUnder` draws them: none for a tray of one pile or a fan.
#[must_use]
pub fn tray_boxes(held: &Held, space: TraySpace, size: Size) -> Vec<Rect> {
    let (per_point, _) = stage_of(&space, size);
    let laid = laid_out(held, space, size);
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
    landing_at(held, space, size, point, carried, |_| 0.0)
}

pub(super) fn landing_at(
    held: &Held,
    space: TraySpace,
    size: Size,
    point: Vec2,
    carried: ObjectId,
    scroll: impl Fn(usize) -> f32,
) -> Option<(usize, usize)> {
    let (per_point, _) = stage_of(&space, size);
    let mut point = point * per_point;
    let laid = laid_out(held, space, size);
    let pile = laid.boxes.iter().position(|area| area.contains(point))?;
    if let (0, Some(library)) = (pile, laid.library) {
        return Some((
            0,
            library_at(&held.piles[0], &laid.column, library, point, carried),
        ));
    }
    let first: usize = held.piles[..pile].iter().map(Vec::len).sum();
    let spots = &laid.spots[first..first + held.piles[pile].len()];
    point.y += scroll(pile) * per_point;
    let nearest_row = if laid.grids.get(pile).is_some_and(|grid| grid.limit > 0.0) {
        spots.iter().map(|spot| spot.y).min_by(|a, b| {
            let middle = point.y - height_of(&space, spots[0].width) / 2.0;
            (a - middle).abs().total_cmp(&(b - middle).abs())
        })
    } else {
        None
    };
    Some((
        pile,
        spots
            .iter()
            .filter(|spot| {
                nearest_row.is_some_and(|row| spot.y < row)
                    || (nearest_row.is_none_or(|row| spot.y <= row)
                        && spot.x + spot.width / 2.0 < point.x)
            })
            .count(),
    ))
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
    let height = height_of(space, width);
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
            let depth = space.tuning.depth - spot.drawn as f32;
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
pub fn tray<T>(piles: &[&[T]], mut space: TraySpace, camera: &Camera) -> Vec<Pose> {
    let (per_point, stage_width) = stage_of(&space, camera.window.size);
    space.grid_target *= per_point;
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
    let laid = laid_out(held, space, camera.window.size);
    let cards: Vec<_> = (held.piles.iter().flatten().copied())
        .zip(poses(&laid, &space, stage_width, camera))
        .collect();
    let backs = space.tuning.put_back.deck_backs;
    let library = (laid.library.into_iter())
        .flat_map(|area| {
            (0..backs).map(move |index| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a deck is drawn with a few backs"
                )]
                let under = (backs - 1 - index) as f32;
                let offset = space.tuning.put_back.deck_offset * under;
                let at = (area.x + offset.x, area.y + offset.y, area.width);
                screen_pose(
                    camera,
                    &space,
                    stage_width,
                    at,
                    0.0,
                    space.tuning.depth + 1.0 + under,
                )
            })
        })
        .collect();
    let (per_point, _) = stage_of(&space, camera.window.size);
    let mut grids = laid.grids;
    for (index, grid) in grids.iter_mut().enumerate() {
        grid.name = format!("tray grid {}", index + 1);
        grid.labels = cards[grid.cards.clone()]
            .iter()
            .map(|(slot, _)| format!("card box #{}", slot.0))
            .collect();
        grid.region = kit_base::geometry::zoom_rect(grid.region, (Vec2::ZERO, 1.0 / per_point));
        grid.limit /= per_point;
    }
    TrayPoses {
        cards,
        library,
        grids,
    }
}
