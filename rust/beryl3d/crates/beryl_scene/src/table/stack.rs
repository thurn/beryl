//! The stack's placement: its spells and its abilities, bottom first, each a step nearer the
//! viewer and higher than the item beneath it (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectId, ObjectView, Side, StackItem, ZoneName};

use super::Layout;
use super::place::{Placement, Rest, turn};
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
/// none takes the next of `numbers`, and one with neither is unnumbered.
pub(super) fn on_stack<'a>(
    stack: impl Iterator<Item = (&'a StackItem, Option<u32>)>,
    numbers: &[u32],
) -> Vec<OnStack> {
    let mut numbers = numbers.iter().copied();
    stack
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
        .collect()
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

/// The stack's items, `stack` bottom first, raised above the table at its right: each one
/// nearer the viewer and higher than the one beneath. A spell is its card, one of `spells`;
/// an ability is drawn as a card too, showing its source's face, and rests by number in
/// `placement.abilities`.
pub(super) fn place_stack(
    placement: &mut Placement,
    spells: &[&ObjectView],
    stack: &[OnStack],
    layout: &Layout,
) {
    placement.abilities.clear();
    // `spells` lists the spells of `stack` in its order, leaving out any whose card the table
    // does not hold.
    let mut spells = spells.iter().peekable();
    let mut index = 0;
    for item in stack {
        let (turn, side) = match *item {
            OnStack::Spell(slot) => {
                let Some(object) = spells.next_if(|object| object.slot == slot) else {
                    continue;
                };
                (turn(object), object.controller)
            }
            OnStack::Ability { side, .. } => (0.0, side),
            OnStack::Unnumbered => {
                index += 1;
                continue;
            }
        };
        let pose = Pose {
            position: layout.stack + layout.stack_step * index as f32,
            turn,
            ..Pose::default()
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
        index += 1;
    }
    placement.abilities.sort_unstable_by_key(|(id, _)| *id);
}
