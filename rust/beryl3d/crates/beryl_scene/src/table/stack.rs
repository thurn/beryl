//! The stack's placement: its spells and its abilities, bottom first, at the right of the screen
//! and placed relative to the camera, as the web interface's `stackPlace`
//! (`ui/src/game/stackLayout.ts`) places them (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, StackItem, ZoneName};

use glam::{Quat, Vec2};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, StackData};
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

/// Where the item at `index`, bottom first, of a stack of `count` stands. The top item's foot
/// stays put; each item beneath peeks out `data.step` above the one over it, closer together
/// once they would reach past `data.reach`; and a stack too tall to fit under `data.top`
/// shrinks, all its items alike, about its column's middle.
fn stack_pose(index: usize, count: usize, data: &StackData, camera: &Camera) -> Pose {
    let size = camera.window.size;
    let aspect = size.width / size.height;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a stack holds far fewer than 2^24 items"
    )]
    let (gaps, beneath, index) = (
        count.saturating_sub(1) as f32,
        (count - 1 - index) as f32,
        index as f32,
    );
    let step = if gaps > 0.0 {
        data.step.min(data.reach / gaps)
    } else {
        0.0
    };
    let tall = data.height * 2.0;
    let wide = tall * CARD_WIDTH / CARD_HEIGHT;
    let scale = ((data.top - data.foot) / (tall + gaps * step)).min(1.0);
    let column = aspect - data.right - wide / 2.0;
    let centre = Vec2::new(
        column - beneath * step * scale * data.lean,
        data.foot + (tall / 2.0 + beneath * step) * scale,
    );
    let ndc = Vec2::new(centre.x / aspect, centre.y);
    let depth = data.depth - index * data.step_depth;
    camera.screen_pose(ndc, depth, Quat::IDENTITY, data.height * scale)
}
