//! Records when the source of a stacked or delayed ability transforms.
//!
//! Implements: none. Stores whether the source has transformed for the rules.

use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::reference::Permanent;
use crate::state::{ObjectRef, StackEntry, StackItem, State};
use crate::table::GameTable;

/// An ability's controller and whether its source has transformed while it was waiting.
/// The high bit records the transform. A spell's controller is a plain `Seat`.
/// Before a transform, hashing the packed byte writes the original seat discriminant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityController(u8);

impl AbilityController {
    const TRANSFORMED: u8 = 1 << 7;

    #[inline]
    pub fn seat(self) -> Seat {
        let mask = Seat::BOTH.len().next_power_of_two() - 1;
        Seat::BOTH[usize::from(self.0) & mask]
    }

    fn mark_transformed(&mut self) {
        self.0 |= Self::TRANSFORMED;
    }

    fn source_has_transformed(self) -> bool {
        self.0 & Self::TRANSFORMED != 0
    }
}

impl From<Seat> for AbilityController {
    #[inline]
    fn from(seat: Seat) -> Self {
        Self(seat.get())
    }
}

impl<L: Layout> StackEntry<L> {
    /// Whether its source has transformed since this ability started waiting
    /// (see CR 701.27g).
    pub fn source_has_transformed(&self) -> bool {
        match self.item {
            StackItem::Activated { of } => of.controller.source_has_transformed(),
            StackItem::Triggered { of, .. } => of.controller.source_has_transformed(),
            StackItem::Spell { .. } => false,
        }
    }

    /// Records that this ability's source has transformed (see CR 701.27g).
    pub fn mark_source_transformed(&mut self) {
        match &mut self.item {
            StackItem::Activated { of } => of.controller.mark_transformed(),
            StackItem::Triggered { of, .. } => of.controller.mark_transformed(),
            StackItem::Spell { .. } => {}
        }
    }
}

impl<L: Layout> State<L> {
    /// Records a transform for the abilities already waiting on the same permanent.
    /// See [CR 701.27g]: "the permanent does so only if it hasn’t transformed or converted
    /// since the ability was put onto the stack." A delayed ability counts from creation.
    ///
    /// [CR 701.27g]: https://yawgatog.com/resources/magic-rules/#R70127g
    pub(crate) fn mark_source_transformed(&mut self, t: &GameTable, permanent: Permanent<L>) {
        let source = Some(ObjectRef::from(permanent));
        for entry in self.stack.as_mut_slice() {
            if entry.source() == source {
                entry.mark_source_transformed();
            }
        }
        for trigger in self.delayed.as_mut_slice() {
            if trigger.of.source == source {
                trigger.of.controller.mark_transformed();
            }
        }
        for trigger in self.triggers.as_mut_slice() {
            let of = trigger.of;
            if of.source == source
                && of
                    .ability
                    .of_face(t.def(of.card).face(of.face))
                    .trigger()
                    .is_some_and(|trigger| trigger.delayed)
            {
                trigger.of.controller.mark_transformed();
            }
        }
    }
}
