//! The stack's placement: its spells and its abilities, bottom first, at the right of the screen
//! and placed relative to the camera, as the web interface's `stackPlace`
//! (`ui/src/game/stackLayout.ts`) places them (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectId, ObjectView, RowName, Side, StackItem, ZoneName};

use glam::{Quat, Vec2, Vec3};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, Layout, RowArea, StackData};
use crate::agent::hit;
use crate::data::audit;
use crate::frame::Place;
use crate::pose::Pose;

/// An item of the stack as placement reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OnStack {
    /// The spell whose card is in this slot.
    Spell(ObjectId),
    /// The ability with this number, on its controller's side.
    Ability { id: u32, side: Side },
    /// An ability nothing numbered, which is not placed but takes its place on the stack, so
    /// that the items above it stand where they would if it were numbered.
    Unnumbered,
}

/// The items of `stack`, in order, each given with its number if it has one. An ability with
/// none takes the next of `numbers`, and one with neither is unnumbered. A spell resolving is
/// the top item: the game takes it off the stack as it begins to resolve, but its card, one of
/// `objects`, stays in the stack's zone until it is put into its next zone.
pub(crate) fn on_stack<'a>(
    stack: impl Iterator<Item = (&'a StackItem, Option<u32>)>,
    numbers: &[u32],
    objects: impl Iterator<Item = &'a ObjectView>,
) -> Vec<OnStack> {
    let mut numbers = numbers.iter().copied();
    let mut items: Vec<OnStack> = stack
        .map(|(item, id)| match item {
            StackItem::Spell { slot, .. } => OnStack::Spell(*slot),
            StackItem::Ability { controller, .. } => match id.or_else(|| numbers.next()) {
                Some(id) => OnStack::Ability {
                    id,
                    side: *controller,
                },
                None => OnStack::Unnumbered,
            },
        })
        .collect();
    for object in objects.filter(|object| object.zone == ZoneName::Stack) {
        if !items.contains(&OnStack::Spell(object.slot)) {
            items.push(OnStack::Spell(object.slot));
        }
    }
    items
}

/// The slots of the spells on `stack`, in order.
pub(super) fn spells(stack: &[OnStack]) -> Vec<ObjectId> {
    stack
        .iter()
        .filter_map(|item| match item {
            OnStack::Spell(slot) => Some(*slot),
            OnStack::Ability { .. } | OnStack::Unnumbered => None,
        })
        .collect()
}

/// The stack's items, `stack` bottom first, at the right of the screen: the top item in front,
/// and each item beneath it peeking out above and to its left. A spell is its card, one of
/// `spells`; an ability is drawn as a card too, showing its source's face, and rests by number
/// in `placement.abilities`.
pub(super) fn place_stack(
    placement: &mut Placement,
    spells: &[&ObjectView],
    stack: &[OnStack],
    (data, camera): (&StackData, &Camera),
) {
    placement.abilities.clear();
    // `spells` lists the spells of `stack` in its order, leaving out any whose card the table
    // does not hold.
    let mut spells = spells.iter().peekable();
    for (index, item) in stack.iter().enumerate() {
        let (turn, side) = match *item {
            OnStack::Spell(slot) => {
                let Some(object) = spells.next_if(|object| object.slot == slot) else {
                    continue;
                };
                (turn(object), object.controller)
            }
            OnStack::Ability { side, .. } => (0.0, side),
            OnStack::Unnumbered => continue,
        };
        let pose = Pose {
            turn,
            ..stack_pose(index, stack.len(), data, camera)
        };
        let rest = Rest {
            pose,
            place: Place::Card {
                zone: ZoneName::Stack,
                side,
                row: None,
            },
            visible: true,
            copies: 1,
        };
        match *item {
            OnStack::Spell(slot) => placement.set(slot, rest),
            OnStack::Ability { id, .. } => placement.abilities.push((id, rest)),
            OnStack::Unnumbered => {}
        }
    }
    placement.abilities.sort_unstable_by_key(|(id, _)| *id);
}

/// The smallest part of its full size a stack in rows shrinks its items to before it lets its
/// rows reach further left than the leaning stack would. Fifty items, the tallest stress board,
/// fit in a window 900 points high at about 0.42.
const SMALLEST_IN_ROWS: f32 = 0.4;

/// The full size of an item, as its height and its width in units of half the window's height.
fn full_size(data: &StackData) -> (f32, f32) {
    let tall = data.height * 2.0;
    (tall, tall * CARD_WIDTH / CARD_HEIGHT)
}

/// The middle of the full-size column across, in units of half the window's height from the
/// window's middle.
fn column(data: &StackData, camera: &Camera) -> f32 {
    let size = camera.window.size;
    let (_, wide) = full_size(data);
    size.width / size.height - data.right - wide / 2.0
}

/// How far left of its column's middle the stack's items ever reach, in units of half the
/// window's height: the left edge of the bottom item of a leaning stack whose items peek out
/// their whole `data.reach`. A shorter leaning stack reaches less far, and a taller stack
/// stands in rows that keep within it (`in_rows`).
fn furthest_reach(data: &StackData) -> f32 {
    let (tall, wide) = full_size(data);
    let scale = ((data.top - data.foot) / (tall + data.reach)).min(1.0);
    (data.reach * data.lean + wide / 2.0) * scale
}

/// The area of the battlefield row `row` on `side`'s half of the table, and its height on the
/// table. As the web interface ends the permanents' rows left of the stack (`ROOM.end` in
/// `ui/src/game/tableRows.ts`), a row that the stack's band of the screen covers so far that its
/// cards keep no uncovered strip the reachable check's square high above or below it ends a
/// `layout.gap` left of the furthest the stack's items ever reach (`furthest_reach`). That end
/// does not depend on how tall the stack is, so the row's cards stay where they are as the stack
/// grows. The other rows keep their whole area.
pub(crate) fn row_area(
    (layout, camera): (&Layout, &Camera),
    side: Side,
    row: RowName,
) -> (RowArea, f32) {
    let area = match row {
        RowName::Creatures => layout.creatures,
        RowName::Lands => layout.lands,
        RowName::Others => layout.others,
    };
    let y = match side {
        Side::You => area.y,
        Side::Opponent => -area.y,
    };
    let data = &layout.stack;
    let window = camera.window.size;
    let on_screen = |ndc: f32| f32::midpoint(1.0, -ndc) * window.height;
    let (stack_top, stack_foot) = (on_screen(data.top), on_screen(data.foot));
    // The band a tapped card of the row lies across, the narrower of its two.
    let height = |along: f32| camera.to_screen(Vec3::new(0.0, y + along, 0.0)).y;
    let (near, far) = (height(-CARD_WIDTH / 2.0), height(CARD_WIDTH / 2.0));
    let uncovered = (stack_top - near.min(far)).max(near.max(far) - stack_foot);
    // The reachable check finds an uncovered square only in whole cells of its grid.
    if uncovered >= audit::TARGET_POINTS + 2.0 * hit::CELL {
        return (area, y);
    }
    let middle = column(data, camera);
    let aspect = window.width / window.height;
    let left = f32::midpoint((middle - furthest_reach(data)) / aspect, 1.0) * window.width;
    // Along a line of the table at one height, a point's place across the screen is linear in
    // its x. A card's right edge runs the length of an untapped card, and the end of it that
    // stands further right on screen meets the stack's reach first.
    let right = [-CARD_HEIGHT / 2.0, CARD_HEIGHT / 2.0]
        .map(|along| {
            let point = Vec3::new(0.0, y + along, 0.0);
            (left - camera.to_screen(point).x) / camera.points_per_millimetre(point)
        })
        .into_iter()
        .fold(f32::INFINITY, f32::min);
    let area = RowArea {
        right: area.right.min(right - layout.gap),
        ..area
    };
    (area, y)
}

/// Where the item at `index`, bottom first, of a stack of `count` stands. The top item's foot
/// stays put; each item beneath peeks out `data.step` above the one over it, closer together
/// once they would reach past `data.reach`; and a stack too tall to fit under `data.top`
/// shrinks, all its items alike, about its column's middle.
///
/// An item beneath is reachable through the strip it shows at its left, `lean` times as wide as
/// it peeks out above. A stack so tall that the strip would be narrower than the reachable
/// check's square (`audit::TARGET_POINTS`) stands in rows instead (`in_rows`).
fn stack_pose(index: usize, count: usize, data: &StackData, camera: &Camera) -> Pose {
    let size = camera.window.size;
    let aspect = size.width / size.height;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (gaps, beneath) = (count.saturating_sub(1) as f32, (count - 1 - index) as f32);
    let step = if gaps > 0.0 {
        data.step.min(data.reach / gaps)
    } else {
        0.0
    };
    let (tall, _) = full_size(data);
    let column = column(data, camera);
    let scale = ((data.top - data.foot) / (tall + gaps * step)).min(1.0);
    // The window is 2 units of normalized device coordinates high.
    let target = audit::TARGET_POINTS * 2.0 / size.height;
    let (centre, height) = if gaps == 0.0 || step * scale * data.lean >= target {
        let centre = Vec2::new(
            column - beneath * step * scale * data.lean,
            data.foot + (tall / 2.0 + beneath * step) * scale,
        );
        (centre, data.height * scale)
    } else {
        in_rows(count - 1 - index, count, target * data.spare, data, column)
    };
    let ndc = Vec2::new(centre.x / aspect, centre.y);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let depth = data.depth - index as f32 * data.step_depth;
    camera.screen_pose(ndc, depth, Quat::IDENTITY, height)
}

/// The middle and the height of the item `beneath` items under the top of a stack of `count`
/// that stands in rows. Each row stands `pitch` above the one in front of it, and each item of
/// a row `pitch` to the left of the one in front of it, so every item beneath keeps a corner
/// `pitch` square uncovered at its top left. The top item stands at the leaning stack's column,
/// and the rows hold as many items each as lets the items be largest while the rows fit under
/// `data.top` and reach left no further than the leaning stack ever does (`furthest_reach`),
/// which keeps them off the permanents' rows (`row_area`). A stack whose items would shrink below
/// `SMALLEST_IN_ROWS` of their full size makes its rows longer instead.
fn in_rows(beneath: usize, count: usize, pitch: f32, data: &StackData, column: f32) -> (Vec2, f32) {
    let (tall, wide) = full_size(data);
    let room = data.top - data.foot;
    let reach = furthest_reach(data);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let scale_for = |per_row: usize| {
        let rows = count.div_ceil(per_row);
        let across = (reach - (per_row - 1) as f32 * pitch) / (wide / 2.0);
        let up = (room - (rows - 1) as f32 * pitch) / tall;
        across.min(up).min(1.0)
    };
    // A row of more items than this leaves none of them room to the right of the reach.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the quotient is small and positive"
    )]
    let longest = ((reach / pitch) as usize + 1).min(count);
    let (mut per_row, mut scale) = (1..=longest)
        .map(|per_row| (per_row, scale_for(per_row)))
        .fold((1, f32::NEG_INFINITY), |best, next| {
            if next.1 > best.1 { next } else { best }
        });
    if scale < SMALLEST_IN_ROWS {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the quotient is small and positive"
        )]
        let most_rows = ((room - tall * SMALLEST_IN_ROWS) / pitch).max(0.0) as usize + 1;
        per_row = count.div_ceil(most_rows);
        scale = SMALLEST_IN_ROWS;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (row, place) = ((beneath / per_row) as f32, (beneath % per_row) as f32);
    let centre = Vec2::new(
        column - place * pitch,
        data.foot + tall / 2.0 * scale + row * pitch,
    );
    (centre, data.height * scale)
}
