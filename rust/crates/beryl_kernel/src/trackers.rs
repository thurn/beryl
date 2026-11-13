//! The Trackers store: what happened this turn that a card of the game reads.
//!
//! A tracker is noted by the rules as the thing happens ([`State::track`]) and read later
//! in the same turn ([`State::tracked`]). It is no memo: a memo is written once and lasts
//! as long as its object, where the turn resets a tracker whole. And it is kept only in a
//! game with a card that reads it. Each kind names its [`Capability`], which
//! [`GameTable::new`] finds once from the game's cards, so in every other game the write is
//! one AND of the table's capabilities and the store stays empty.
//!
//! Built so far (by-w8gv.7.8): the kinds keyed by permanent, each one set of slots, both of
//! this turn. No card yet reads a count of this game, or one kept for a player: such a kind
//! joins with a shape and a span of its own, as its first reader does.

use crate::bitboard::Bitboard;
use crate::fit::Misfit;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Small};
use crate::observation::Observation;
use crate::relayout::{Relayout, bits};
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::{Capability, GameTable};

/// The kinds of tracker keyed by permanent: one list, from which [`Tracker`], the order of
/// the store's sets and each kind's capability and words are written, so a new kind is an
/// entry here and its reader. Each entry names the [`Capability`] a game's cards must have
/// for the kind to be kept, and what it says of a permanent in a save's description. A
/// `const fn` cannot enumerate an enum's variants.
macro_rules! trackers {
    ($($(#[$doc:meta])* $variant:ident = $words:literal),* $(,)?) => {
        /// A kind of tracker keyed by permanent: the closed list. Each is a fact about a
        /// permanent that ends with the turn, or as the permanent leaves the battlefield
        /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that
        /// moves from one zone to another becomes a new object with no memory of, or
        /// relation to, its previous existence.").
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Tracker {
            $($(#[$doc])* $variant),*
        }

        impl Tracker {
            /// Every kind, in the order of the store's sets.
            pub const ALL: [Tracker; [$(Tracker::$variant),*].len()] = [$(Tracker::$variant),*];

            /// What a game's cards must be able to do for it to be kept, the capability of
            /// its own name: with no card that reads it, nothing writes it.
            #[inline]
            pub const fn capability(self) -> Capability {
                match self {
                    $(Tracker::$variant => Capability::$variant),*
                }
            }

            /// What it says of a permanent, as a save's description words it.
            fn words(self) -> &'static str {
                match self {
                    $(Tracker::$variant => $words),*
                }
            }
        }
    };
}

trackers! {
    /// The permanent entered the battlefield this turn. The rules never ask; a static
    /// ability may, in its condition (`beryl_card`'s `Condition::EnteredThisTurn`).
    EnteredThisTurn = "entered this turn",
    /// A loyalty ability of the permanent was activated this turn
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)): "A player may
    /// activate a loyalty ability of a permanent they control any time they have priority
    /// and the stack is empty during a main phase of their turn, but only if no player has
    /// previously activated a loyalty ability of that permanent that turn."
    LoyaltyActivated = "had a loyalty ability activated this turn",
}

/// The Trackers store: for each kind, the permanents it is noted of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Trackers<L: Layout = Small> {
    sets: [Bitboard<L::Slot>; Tracker::ALL.len()],
}

/// The store is 64 bytes in [`Small`], a set of slots for each kind: the state's room (the
/// crate's contract, "Stores") counts on it.
const _: () = assert!(size_of::<Trackers<Small>>() == 32 * Tracker::ALL.len());

impl<L: Layout> Trackers<L> {
    /// The next turn begins: nothing has happened in it. Every kind so far is of this turn.
    #[inline]
    pub(crate) fn begin_turn(&mut self) {
        *self = Self::empty();
    }

    /// Notes `tracker` of the permanent in `slot`, or no longer.
    #[inline]
    pub(crate) fn set(&mut self, tracker: Tracker, slot: L::Slot, on: bool) {
        let set = &mut self.sets[tracker as usize];
        if on {
            set.insert(slot);
        } else {
            set.remove(slot);
        }
    }
}

impl<L: Layout> Store<L> for Trackers<L> {
    #[inline]
    fn empty() -> Self {
        Trackers {
            sets: [Bitboard::EMPTY; Tracker::ALL.len()],
        }
    }

    /// None. A tracker is about a permanent, and the zone move ends a permanent's where it
    /// unlists it from the battlefield, with two bits cleared. Through the keyed slots,
    /// every permanent that entered this turn would leave by the stores' rare path.
    #[inline]
    fn keys(&self) -> Bitboard<L::Slot> {
        Bitboard::EMPTY
    }

    /// None: a tracker is a fact about one permanent.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        Bitboard::EMPTY
    }

    /// Ends what is tracked of the permanent in `slot`, as it leaves the battlefield. The
    /// state has nothing to settle: the slot was no key, and the layers read no tracker.
    #[inline]
    fn forget(&mut self, slot: L::Slot) -> Ended {
        for set in &mut self.sets {
            set.remove(slot);
        }
        Ended::default()
    }

    /// Nothing: a tracker lasts for the turn, which the kernel itself begins
    /// ([`State::begin_turn`]).
    #[inline]
    fn expire(&mut self, _: Duration) -> Ended {
        Ended::default()
    }

    /// Always: a set of slots has a place for every object of its layout, and the objects'
    /// own fit says whether a layout has the slots.
    fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        Ok(())
    }

    fn check(&self, s: &State<L>, t: &GameTable) -> Result<(), Violation> {
        let battlefield = s.in_zone(Zone::Battlefield);
        for tracker in Tracker::ALL {
            let set = self.sets[tracker as usize];
            let stray = (set & !battlefield).iter().next();
            check(
                stray.is_none(),
                "a tracker of an object off the battlefield",
                stray.map(|slot| At::Slot(slot.index())),
            )?;
            check(
                set.is_empty() || t.can(tracker.capability()),
                "a tracker kept that no card of the game reads",
                None,
            )?;
        }
        Ok(())
    }

    /// The first permanent a tracker is noted of: beginning the turn again would begin it
    /// with none. A checkpoint is taken as the upkeep begins, when nothing has yet entered
    /// and no loyalty ability has been activated, so there is none to save.
    fn lingering(&self, s: &State<L>, t: &GameTable) -> Option<String> {
        Tracker::ALL.into_iter().find_map(|tracker| {
            let slot = self.sets[tracker as usize].iter().next()?;
            Some(format!("{} {}", t.def(s.card(slot)).name, tracker.words()))
        })
    }
}

impl<L: Layout> Relayout for Trackers<L> {
    type In<M: Layout> = Trackers<M>;

    fn relayout<M: Layout>(&self) -> Trackers<M> {
        Trackers {
            sets: self.sets.map(bits),
        }
    }
}

/// What the Trackers store holds about one permanent, to compare two by
/// ([`State::signature`]): the kinds noted of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Tracked(u8);

/// A bit for each kind of the list.
const _: () = assert!(Tracker::ALL.len() <= u8::BITS as usize);

impl Tracked {
    /// Whether `tracker` is noted of the permanent.
    #[inline]
    pub fn has(self, tracker: Tracker) -> bool {
        self.0 & (1 << (tracker as u8)) != 0
    }
}

impl<L: Layout> State<L> {
    /// Notes `tracker` of the permanent in `slot`, for the rest of the turn or until it
    /// leaves the battlefield: the rules call it as the thing happens. In a game no card of
    /// which reads the tracker it writes nothing, at the cost of one AND. No event: it is
    /// part of what the rules were doing, and the layers read no tracker.
    #[inline]
    pub fn track(&mut self, t: &GameTable, tracker: Tracker, slot: L::Slot) {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        if t.can(tracker.capability()) {
            self.trackers.set(tracker, slot, true);
        }
    }

    /// Whether `tracker` is noted of the permanent in `slot`: a bit tested. Never, in a
    /// game no card of which reads it.
    #[inline]
    pub fn tracked(&self, tracker: Tracker, slot: L::Slot) -> bool {
        self.trackers.sets[tracker as usize].contains(slot)
    }

    /// Everything tracked of the permanent in `slot`: for [`State::signature`], tools,
    /// tests and a view. The rules ask by kind.
    #[inline]
    pub fn trackers(&self, slot: L::Slot) -> Tracked {
        let mut found = 0;
        for tracker in Tracker::ALL {
            found |= u8::from(self.tracked(tracker, slot)) << (tracker as u8);
        }
        Tracked(found)
    }
}

impl<L: Layout> Observation<'_, L> {
    /// Whether `tracker` is noted of the permanent in `slot`. Every player knows: what a
    /// tracker notes happened on the battlefield or the stack, where all could see it
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Public zones are
    /// zones in which all players can see the cards' faces").
    pub fn tracked(&self, tracker: Tracker, slot: L::Slot) -> bool {
        self.shown().tracked(tracker, slot)
    }
}

#[cfg(test)]
mod tests;
