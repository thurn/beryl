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
//! The phone's tray takes its bands from the layout profile. The toolkit fits the
//! cards and provides the viewport for rows and columns that scroll.

use beryl_protocol::protocol::ObjectId;
use glam::{Quat, Vec2};
use std::ops::Range;

use super::Camera;
use kit_base::data::Card;
use kit_base::data::profiles::TrayTuning;
use kit_base::pose::Pose;
use kit_space::arrange::screen::{FanShape, ScreenFit, ScreenGroup, ScreenLayout};
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
    /// Extra spacing for overlap in the phone profile, in stage pixels.
    pub least_step: f32,
    /// The minimum press square, in logical points.
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
    rows: bool,
    pub name: String,
    pub labels: Vec<String>,
}

/// A tray laid out on the stage, as the web's `Laid`: where each card lies, in the order of the
/// piles and then of their cards; the box of each pile of a tray of two, where a card let go
/// lands in it; and for a put-back, where the library is drawn and the top edge of each place
/// in its column, the deepest first, as many as the library's pile can come to hold.
struct Laid {
    spots: Vec<Spot>,
    boxes: Vec<Rect>,
    library: Option<Rect>,
    column: Vec<f32>,
    grids: Vec<TrayGrid>,
}

impl Laid {
    fn of(spots: Vec<Spot>, boxes: Vec<Rect>) -> Self {
        Self {
            spots,
            boxes,
            library: None,
            column: Vec::new(),
            grids: Vec::new(),
        }
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

impl TraySpace {
    fn press(&self) -> f32 {
        // Leave room for rounding as the camera projects the layout back to the screen.
        self.grid_target + 0.01
    }

    fn fit(&self) -> ScreenFit {
        ScreenFit {
            aspect: self.card.height / self.card.width,
            most: self.most,
            least: self.least,
            gap: self.gap,
            press: self.press().max(self.least_step),
        }
    }

    fn region(&self, (left, wide): (f32, f32)) -> Rect {
        Rect {
            x: left,
            y: self.y,
            width: wide.max(0.0),
            height: self.height.max(0.0),
        }
    }
}

/// One pile of cards centred in the tray band.
fn rows(count: usize, space: &TraySpace, span: (f32, f32)) -> Laid {
    let mut laid = Laid::of(Vec::with_capacity(count), Vec::new());
    laid.grid(space.fit().rows(count, space.region(span)));
    laid
}

impl Laid {
    fn grid(&mut self, layout: ScreenLayout) {
        let start = self.spots.len();
        self.spots.extend(
            layout
                .items
                .into_iter()
                .enumerate()
                .map(|(index, item)| Spot {
                    x: item.rect.x,
                    y: item.rect.y,
                    width: item.rect.width,
                    turn: item.turn,
                    drawn: start + index,
                }),
        );
        self.grids.push(TrayGrid {
            cards: start..self.spots.len(),
            region: layout.viewport,
            limit: layout.scroll,
            rows: layout.rows,
            offset: 0.0,
            name: String::new(),
            labels: Vec::new(),
        });
    }
}

/// The dealt order sets the placement and draw order. The result follows pile order.
fn fan(piles: &[Vec<ObjectId>], order: &[ObjectId], space: &TraySpace, span: (f32, f32)) -> Laid {
    let mut laid = Laid::of(Vec::with_capacity(order.len()), Vec::new());
    let shape = space.tuning.fan;
    laid.grid(space.fit().fan(
        order.len(),
        space.region(span),
        FanShape {
            step: shape.step,
            drop: shape.drop,
            turn: shape.turn,
        },
    ));
    laid.spots = in_order(piles, order, |index| laid.spots[index]);
    laid
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
    let groups: Vec<_> = counts
        .iter()
        .enumerate()
        .map(|(index, &count)| ScreenGroup {
            count,
            room: rooms.get(index).copied().flatten().unwrap_or(total),
        })
        .collect();
    let band = space.region(span);
    let second = space.second.map(|second| Rect {
        y: second.y,
        height: second.height.max(0.0),
        ..band
    });
    let mut laid = Laid::of(Vec::with_capacity(total), Vec::with_capacity(groups.len()));
    for group in space.fit().groups(&groups, band, second, space.heading) {
        laid.boxes.push(group.bounds);
        laid.grid(group.layout);
    }
    laid
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
    let pile = laid.boxes.iter().position(|area| {
        area.width > 0.0
            && area.height > 0.0
            && (area.x..=area.x + area.width).contains(&point.x)
            && (area.y..=area.y + area.height).contains(&point.y)
    })?;
    let grid = &laid.grids[pile];
    if grid.region.width <= 0.0 || grid.region.height <= 0.0 {
        return None;
    }
    point.y += (scroll(pile) * per_point).clamp(0.0, grid.limit);
    if let (0, Some(library)) = (pile, laid.library) {
        return Some((
            0,
            library_at(&held.piles[0], &laid.column, library, point, carried),
        ));
    }
    let first: usize = held.piles[..pile].iter().map(Vec::len).sum();
    let spots = &laid.spots[first..first + held.piles[pile].len()];
    let nearest_row = if grid.rows {
        spots.iter().map(|spot| spot.y).min_by(|a, b| {
            let middle = point.y - space.fit().size(spots[0].width).y / 2.0;
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
    let height = space.fit().size(width).y;
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
