//! Phasing (see [CR 702.26]): the mutators that phase permanents out and in.
//!
//! A phased-out permanent keeps its slot, its zone (the battlefield), its incarnation and
//! everything the stores hold about it: its counters, memos, effects and attachments. It leaves
//! the battlefield list, its owner's battlefield zone set and so the layer results, and joins
//! the state's set of phased-out permanents. Everything that walks the battlefield or reads a
//! zone set therefore passes over it, and since nothing changes zone, nothing triggers as though
//! it left or entered (see [CR 702.26d]): "The phasing event doesn't actually cause a permanent to
//! change zones or control, even though it's treated as though it's not on the battlefield and
//! not under its controller's control while it's phased out. Zone-change triggers don't trigger
//! when a permanent phases in or out."
//!
//! A game with nothing phased out reads one empty set, in the untap step and as a permanent
//! leaves the battlefield.
//!
//! Implements: none. The mutators update the battlefield and phased-out records together.
//!
//! [CR 702.26]: https://yawgatog.com/resources/magic-rules/#R70226
//! [CR 702.26d]: https://yawgatog.com/resources/magic-rules/#R70226d

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::memos::Memo;
use crate::observation::Observation;
use crate::reference::Permanents;
use crate::state::{Flag, State, Zone};

/// Phased-out slots selected to phase in. Use within the current instruction.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PhasedOut<L: Layout>(Bitboard<L::Slot>);

impl<L: Layout> PhasedOut<L> {
    /// Whether there are any permanents to phase in.
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// The selected slots, for a batch event or a query.
    #[inline]
    pub fn slots(self) -> Bitboard<L::Slot> {
        self.0
    }
}

impl<L: Layout> State<L> {
    /// The phased-out permanents.
    #[inline]
    pub fn phased_out(&self) -> Bitboard<L::Slot> {
        self.phased_out
    }

    /// Phases out the selected permanents and, with them, everything attached to them
    /// (see [CR 702.26g]): "When a permanent phases out, any Auras, Equipment, or Fortifications
    /// attached to that permanent phase out at the same time. This alternate way of phasing out
    /// is known as phasing out “indirectly.”" Each is removed from combat (see [CR 702.26b]): "A
    /// permanent that phases out is removed from combat." No event: nothing triggers on phasing
    /// (see CR 702.26d).
    ///
    /// Each permanent that phases out directly remembers who controlled it if that is not its
    /// base controller ([`Memo::PhasedOutUnder`]), since that player's untap step phases it in
    /// ([`State::phasing_in_for`]).
    ///
    /// # Panics
    ///
    /// In debug builds, while the layer results are stale.
    ///
    /// [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    pub fn phase_out(&mut self, permanents: Permanents<L>) -> Result<(), Full> {
        let slots = permanents.slots();
        let all = self.with_attached(slots, self.in_zone(Zone::Battlefield));
        // See CR 702.26h: "If an object would simultaneously phase out directly and indirectly, it
        // just phases out indirectly." So a permanent phases out directly when what it is
        // attached to, if anything, stays.
        for slot in all.iter() {
            let o = self.object(slot).clone();
            if o.attached_to.is_some_and(|host| all.contains(host)) {
                continue;
            }
            let under = self.controller(slot);
            if under != o.base_controller {
                self.remember(slot, Memo::PhasedOutUnder(under))?;
            }
        }
        // See CR 506.4: "A creature that's removed from combat stops being an attacking,
        // blocking, blocked, and/or unblocked creature." The creatures in combat with it stay
        // in combat, related to nothing, as when a permanent leaves the battlefield.
        for slot in all.iter() {
            let permanent = self.permanent_at(slot).expect("a permanent phasing out");
            self.remove_from_combat(permanent);
        }
        for slot in all.iter() {
            if self.has_preparations() {
                self.remove_prepare_copy(slot)?;
            }
            self.leave_battlefield(slot);
            let o = self.object_mut(slot);
            o.flags.set(Flag::PhasedOut, true);
            let owner = o.pile_seat().index();
            self.zones.members[owner][Zone::Battlefield.index()].remove(slot);
            self.phased_out.insert(slot);
        }
        self.flags.derived_dirty = true;
        Ok(())
    }

    /// Phases in the selected permanents (see [CR 702.26c]): "If a permanent phases
    /// in, its status changes to “phased in.” The game once again treats it as though it
    /// exists." Each is as it was: tapped or untapped, with its counters, attachments and
    /// timestamp. One attached to a permanent that stays phased out phases in unattached
    /// (see [CR 702.26i]): "An Aura, Equipment, or Fortification that phased out directly will phase
    /// in attached to the object or player it was attached to when it phased out, if that object
    /// is still in the same zone or that player is still in the game. If not, that Aura,
    /// Equipment, or Fortification phases in unattached." One whose object left the battlefield
    /// was unattached as it left. No event (see CR 702.26d).
    ///
    /// [CR 702.26c]: https://yawgatog.com/resources/magic-rules/#R70226c
    /// [CR 702.26i]: https://yawgatog.com/resources/magic-rules/#R70226i
    pub fn phase_in(&mut self, permanents: PhasedOut<L>) -> Result<(), Full> {
        let slots = permanents.slots();
        self.phased_out &= !slots;
        for slot in slots.iter() {
            self.enter_battlefield(slot)?;
            let staying = self.phased_out;
            let o = self.object_mut(slot);
            o.flags.set(Flag::PhasedOut, false);
            if o.attached_to.is_some_and(|host| staying.contains(host)) {
                o.attached_to = None;
            }
            let owner = o.pile_seat().index();
            self.zones.members[owner][Zone::Battlefield.index()].insert(slot);
        }
        self.forget_phased_out_under(slots);
        self.flags.derived_dirty = true;
        Ok(())
    }

    /// The phased-out permanents that phase in during `player`'s untap step (see [CR 502.1]):
    /// "all phased-out permanents that the active player controlled when they phased out phase
    /// in." Those that phased out indirectly come with what they are attached to
    /// (see [CR 702.26g]): "An Aura, Equipment, or Fortification that phased out indirectly won't
    /// phase in by itself, but instead phases in along with the permanent it's attached to."
    ///
    /// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
    /// [CR 702.26g]: https://yawgatog.com/resources/magic-rules/#R70226g
    pub fn phasing_in_for(&self, player: Seat) -> PhasedOut<L> {
        let out = self.phased_out;
        if out.is_empty() {
            return PhasedOut(out);
        }
        let direct = out.filter(|slot| {
            let o = self.object(slot);
            !o.attached_to.is_some_and(|host| out.contains(host))
                && self.phased_out_under(slot).unwrap_or(o.base_controller) == player
        });
        PhasedOut(self.with_attached(direct, out))
    }

    /// `slots` and every object among `among` attached to one of them, or to one attached to
    /// one of them, and so on.
    fn with_attached(
        &self,
        slots: Bitboard<L::Slot>,
        among: Bitboard<L::Slot>,
    ) -> Bitboard<L::Slot> {
        let mut all = slots;
        loop {
            let more = among.filter(|slot| {
                !all.contains(slot)
                    && (self.object(slot).attached_to).is_some_and(|host| all.contains(host))
            });
            if more.is_empty() {
                return all;
            }
            all |= more;
        }
    }
}

impl<L: Layout> Observation<'_, L> {
    /// The phased-out permanents: public, as the battlefield is (see [CR 400.2]): "Public zones are
    /// zones in which all players can see the cards' faces".
    ///
    /// [CR 400.2]: https://yawgatog.com/resources/magic-rules/#R4002
    #[inline]
    pub fn phased_out(&self) -> Bitboard<L::Slot> {
        self.shown().phased_out()
    }
}
