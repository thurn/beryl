//! The controller the layer results last gave each permanent, where a static ability made it
//! other than the permanent's base controller.
//! Implements: none. Stores what the rules compare to find a change of control.

use super::{Full, Kinds, Layout, Memo, Queue, Seat, State};
use crate::object::Flag;

impl<L: Layout> State<L> {
    /// Finds each permanent whose controller in the current layer results differs from the
    /// controller the last call found ([`Memo::ControlledBy`], or its base controller
    /// without one), and marks it as not continuously controlled since its controller's most
    /// recent turn began (see [CR 302.6]): "A creature's activated ability with the tap symbol
    /// or the untap symbol in its activation cost can't be activated unless the creature has
    /// been under its controller's control continuously since their most recent turn began. A
    /// creature can't attack unless it has been under its controller's control continuously
    /// since their most recent turn began."
    ///
    /// A control change by a resolving spell or ability marks the permanent as it begins. One
    /// by a static ability has no such moment: Control Magic's begins as the Aura enters
    /// attached and ends as it leaves, and either shows only in the layer results. The rules
    /// call this at each state-based action check, in a game with a card whose static
    /// ability changes control. The results must be current. No event: the mark is part of the control
    /// change.
    ///
    /// # Errors
    ///
    /// [`Full`] if the memo store had no room for a permanent's new controller.
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    pub fn note_control_changes(&mut self) -> Result<(), Full> {
        let mut noted = Ok(());
        for i in 0..self.battlefield.len() {
            let slot = self.battlefield.as_slice()[i];
            let now = self.controller(slot);
            let base = self.base_controller(slot);
            let seen = self.controlled_by(slot).unwrap_or(base);
            if now == seen {
                continue;
            }
            self.object_mut(slot).flags.set(Flag::SummoningSick, true);
            if seen != base {
                let done = self
                    .memos
                    .end(|e| !(e.slot == slot && matches!(e.memo, Memo::ControlledBy(_))));
                self.settle_ended(done);
            }
            if now != base {
                noted = noted.and(self.remember(slot, Memo::ControlledBy(now)));
            }
        }
        noted
    }

    /// The controller the layer results last gave the permanent in `slot`, if that is not its
    /// base controller. With none in the game, one AND.
    #[inline]
    fn controlled_by(&self, slot: L::Slot) -> Option<Seat> {
        let by = |memo| {
            if let Memo::ControlledBy(player) = memo {
                Some(player)
            } else {
                None
            }
        };
        self.memos.find(Kinds::CONTROLLED_BY, slot, by)
    }
}
