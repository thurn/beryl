//! The stack's placement: its spells and its abilities, bottom first, at the right of the screen
//! and placed relative to the camera, as the web interface's `stackPlace`
//! (`ui/src/game/stackLayout.ts`) places them (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, StackItem, ZoneName};

use glam::{Quat, Vec2};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, StackData};
use crate::data::audit;
use crate::frame::Place;
use crate::pose::Pose;

/// An item of the stack as placement reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OnStack {
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
pub(super) fn on_stack<'a>(
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
/// rows reach further left than the leaning stack would.
const SMALLEST_IN_ROWS: f32 = 0.5;

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
    let tall = data.height * 2.0;
    let wide = tall * CARD_WIDTH / CARD_HEIGHT;
    let scale = ((data.top - data.foot) / (tall + gaps * step)).min(1.0);
    let column = aspect - data.right - wide / 2.0;
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
/// `pitch` square uncovered at its top left. The top item stands where it would in the leaning
/// stack, its row runs left no further than the leaning stack reaches, and the items shrink
/// alike so that the rows fit under `data.top`. A stack whose rows would shrink its items below
/// `SMALLEST_IN_ROWS` of their full size makes its rows longer instead.
fn in_rows(beneath: usize, count: usize, pitch: f32, data: &StackData, column: f32) -> (Vec2, f32) {
    let tall = data.height * 2.0;
    let room = data.top - data.foot;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both quotients are small and positive"
    )]
    let (across, most_rows) = (
        (data.reach * data.lean / pitch) as usize + 1,
        ((room - tall * SMALLEST_IN_ROWS) / pitch).max(0.0) as usize + 1,
    );
    let per_row = across.max(count.div_ceil(most_rows));
    let rows = count.div_ceil(per_row);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (row, place, rows) = (
        (beneath / per_row) as f32,
        (beneath % per_row) as f32,
        rows as f32,
    );
    let scale = ((room - (rows - 1.0) * pitch) / tall).min(1.0);
    let centre = Vec2::new(
        column - place * pitch,
        data.foot + tall / 2.0 * scale + row * pitch,
    );
    (centre, data.height * scale)
}
