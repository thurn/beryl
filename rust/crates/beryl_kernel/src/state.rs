//! The game state, stored as its [`Layout`] says: in [`Small`], one flat `Copy` value with
//! fixed capacities and no heap (architecture §4, decisions D3 and D4).

pub use beryl_card::{CounterKind, TurnStep, Zone};

pub use crate::floating::{CastPermission, Continuous, Duration, FloatingEffect};
pub use crate::object::{Counters, Derived, GrantRef, Object, ObjectFlags, ObjectRef};
pub use crate::pending::{CastStep, Casting, Pending, Resolving};
pub use crate::player::{ManaPool, MulliganStatus, Player, Turn};
pub use crate::stack::{Lki, PendingTrigger, StackEntry, TARGETS, Target, TriggerInfo};

use beryl_card::FaceIx;

use crate::bitboard::Bitboard;
use crate::decision::Outcome;
use crate::fixed::Full;
use crate::ids::{CardIx, Seat, Slot, SlotId};
use crate::layout::{Layout, Queue, Small};
use crate::object::OFF_BATTLEFIELD;
use crate::profile::{Limits, Profile};
use crate::rng::Rng;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = Slot::COUNT;
/// Slots reserved for each player's deck: player `p`'s cards sit in
/// `p * DECK_SLOTS .. p * DECK_SLOTS + deck size`.
pub const DECK_SLOTS: usize = 60;
/// The first slot of the shared pool for tokens and spell copies, after both decks'. Of the
/// 255 slots, the decks reserve 120 and the pool has the other 135.
pub const TOKEN_SLOTS: u8 = 2 * DECK_SLOTS as u8;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;
/// The slot of the `i`th card of `player`'s deck.
pub fn deck_slot<S: SlotId>(player: Seat, i: usize) -> S {
    S::from_index(player.index() * DECK_SLOTS + i)
}

/// The slots of the shared pool for tokens and spell copies ([`Layout::TOKEN_POOL`]).
pub fn token_slots<S: SlotId>() -> impl Iterator<Item = S> {
    (usize::from(TOKEN_SLOTS)..S::COUNT).map(S::from_index)
}

/// Zone membership and the zones whose order the rules read (architecture §4.3).
///
/// Transitional: the fields are `pub`; by-w8gv.5.4 makes them private.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Zones<L: Layout = Small> {
    /// Membership by owner, indexed by `Zone as usize` for the owned zones.
    pub members: [[Bitboard<L::Slot>; Zone::OWNED]; 2],
    pub stack: Bitboard<L::Slot>,
    /// Bottom first: the top card is the last.
    pub library: [L::Queue<L::Slot, DECK_SLOTS>; 2],
    /// Bottom first: the top card is the last.
    pub graveyard: [L::Queue<L::Slot, DECK_SLOTS>; 2],
    /// The next battlefield timestamp.
    pub next_timestamp: u16,
}

impl Copy for Zones<Small> {}

/// Counters toward the budgets of the profile (architecture §12.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Budgets {
    /// Decisions offered this turn.
    pub decisions: u16,
    /// State-based checks in a row that performed something.
    pub state_based_iterations: u8,
}

/// Engine bookkeeping that is not game state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags {
    /// A change since `derived` was last computed may have changed a layer result.
    pub derived_dirty: bool,
}

/// The whole game, stored as its [`Layout`] says: as [`Small`], copied with one `memcpy`
/// (architecture §4.1).
///
/// Transitional: every field is `pub` for the one push that moved the state into this crate,
/// so `beryl_engine` builds against it as it did against its own module. Four beads make them
/// private, each field naming its own (the crate's contract: only the kernel writes a
/// field): objects, by-w8gv.5.3; zones, the battlefield, the derived cache, knowledge and
/// listening, by-w8gv.5.4; players, the turn and the game-level fields, by-w8gv.5.7; the
/// stack and the queues, by-w8gv.5.8. The fields keep their order: a state's hash feeds them
/// in it.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct State<L: Layout = Small> {
    /// Private with by-w8gv.5.3.
    pub objects: L::Table<Object<L>, SLOTS>,
    /// Slots on the battlefield, in the order they entered. Private with by-w8gv.5.4.
    pub battlefield: L::Queue<L::Slot, BATTLEFIELD>,
    /// Layer results, by battlefield index. Private with by-w8gv.5.4.
    pub derived: L::Table<Derived, BATTLEFIELD>,
    /// The kinds of event the permanents on the battlefield have triggered abilities for
    /// ([`crate::table::GameTable::listens`] of each), found with the layer results and
    /// current while they are. Private with by-w8gv.5.4.
    pub listening: u16,
    /// Private with by-w8gv.5.4.
    pub zones: Zones<L>,
    /// Private with by-w8gv.5.7.
    pub players: [Player<L>; 2],
    /// Private with by-w8gv.5.7.
    pub turn: Turn,
    /// Private with by-w8gv.5.8.
    pub stack: L::Queue<StackEntry<L>, 64>,
    /// Private with by-w8gv.5.8.
    pub triggers: L::Queue<PendingTrigger<L>, 64>,
    /// Delayed triggered abilities waiting for their moment (CR 603.7), which their
    /// ability's [`beryl_card::Trigger::Delayed`] names. Private with by-w8gv.5.8.
    pub delayed: L::Queue<PendingTrigger<L>, 32>,
    /// Private with by-w8gv.5.8; milestone 4 (by-w8gv.7.1) replaces it with a store.
    pub effects: L::Queue<FloatingEffect<L>, 64>,
    /// Private with by-w8gv.5.8; milestone 4 (by-w8gv.7.1) replaces it with a store.
    pub permissions: L::Queue<CastPermission<L>, 8>,
    /// The value of X the spells that became these permanents were cast with, for their
    /// enters abilities ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)):
    /// "the value of X for that ability is the same as the value of X for that spell". Only
    /// a nonzero X is kept, and only while the permanent stays. Milestone 4 (by-w8gv.7.1)
    /// deletes it.
    pub entered_x: L::Queue<(ObjectRef<L>, u8), 8>,
    /// Private with by-w8gv.5.8.
    pub pending: Pending<L>,
    /// `knowledge[p]` holds the slots whose identity player `p` knows (architecture §13).
    /// Private with by-w8gv.5.4.
    pub knowledge: [Bitboard<L::Slot>; 2],
    /// Private with by-w8gv.5.7.
    pub rng: Rng,
    /// Private with by-w8gv.5.7.
    pub profile: Profile,
    /// Private with by-w8gv.5.7.
    pub budgets: Budgets,
    /// Private with by-w8gv.5.7.
    pub flags: Flags,
    /// Private with by-w8gv.5.7.
    pub outcome: Option<Outcome>,
}

const _: () = assert!(size_of::<State<Small>>() <= 20 * 1024);

// A `Copy` type's `Clone` should be `*self`, which `Small::clone_state` is.
#[allow(clippy::expl_impl_clone_on_copy)]
impl<L: Layout> Clone for State<L> {
    #[inline]
    fn clone(&self) -> Self {
        L::clone_state(self)
    }
}

impl Copy for State<Small> {}

impl<L: Layout> State<L> {
    pub fn object(&self, slot: L::Slot) -> &Object<L> {
        &self.objects[slot.index()]
    }

    /// The game index of the card in a slot. Players read it through
    /// [`crate::Observation::card`], which masks what they do not know.
    pub fn card(&self, slot: L::Slot) -> CardIx {
        self.object(slot).card
    }

    /// The current incarnation of the object in a slot.
    pub fn object_ref(&self, slot: L::Slot) -> ObjectRef<L> {
        ObjectRef {
            slot,
            incarnation: self.object(slot).incarnation,
        }
    }

    /// Whether a reference still names the object it was taken from.
    pub fn is_current(&self, r: ObjectRef<L>) -> bool {
        self.object(r.slot).incarnation == r.incarnation
    }

    pub fn player(&self, player: Seat) -> &Player<L> {
        &self.players[player.index()]
    }

    pub fn turn(&self) -> &Turn {
        &self.turn
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Changes which decisions are offered from here on (architecture §15).
    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
    }

    /// The budgets search plays under below this position ([`Limits::search`]): a player game
    /// lifts its own ([`Limits::PLAYER`]), which a runaway rollout must not inherit.
    pub fn search_limits(&self) -> Limits {
        Limits::search(self.turn.number, self.budgets.decisions)
    }

    /// The game's random streams: a session compares them to tell whether anything random
    /// happened between two positions.
    pub fn rng(&self) -> Rng {
        self.rng
    }

    /// Reseeds the game's random streams, as search does for each copy it plays on so sibling
    /// rollouts diverge (architecture §14).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// The slots a player owns in a zone other than the stack.
    pub fn zone(&self, owner: Seat, zone: Zone) -> Bitboard<L::Slot> {
        self.zones.members[owner.index()][zone as usize]
    }

    /// The cards exiled with the object in `source` by an ability linked to another of its
    /// abilities ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Kept while that object has left its
    /// zone, until a new object arrives in the slot (CR 400.7).
    pub fn exiled_with(&self, source: L::Slot) -> Bitboard<L::Slot> {
        (self.zone(Seat::P0, Zone::Exile) | self.zone(Seat::P1, Zone::Exile))
            .iter()
            .filter(|&x| {
                let o = self.object(x);
                o.flags.has(ObjectFlags::EXILED_WITH) && o.linked == Some(source)
            })
            .collect()
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: Seat) -> &[L::Slot] {
        self.zones.library[player.index()].as_slice()
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: Seat) -> &[L::Slot] {
        self.zones.graveyard[player.index()].as_slice()
    }

    /// Slots on the battlefield, in the order they entered.
    pub fn battlefield(&self) -> &[L::Slot] {
        self.battlefield.as_slice()
    }

    pub fn pending(&self) -> &Pending<L> {
        &self.pending
    }

    /// The value of X the permanent in `slot` was cast with (CR 107.3m), 0 if none.
    pub fn x_of(&self, slot: L::Slot) -> u8 {
        self.entered_x
            .iter()
            .find(|&&(r, _)| r.slot == slot && self.is_current(r))
            .map_or(0, |&(_, x)| x)
    }

    /// Whether `player` knows the identity of the object in `slot`.
    pub fn knows(&self, player: Seat, slot: L::Slot) -> bool {
        self.knowledge[player.index()].contains(slot)
    }

    /// A permanent's characteristics after continuous effects (architecture §9). Reads never
    /// recompute: every path that changes a layer input recomputes before anything reads.
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield, or, in debug builds, if `derived` is stale.
    pub fn derived(&self, slot: L::Slot) -> &Derived {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        let i = self
            .battlefield_position(slot)
            .expect("derived characteristics of a permanent");
        &self.derived[i]
    }

    /// The index of the permanent in `slot` in the battlefield list, if it is on the
    /// battlefield.
    pub fn battlefield_position(&self, slot: L::Slot) -> Option<usize> {
        let i = self.object(slot).battlefield_index;
        (i != OFF_BATTLEFIELD).then_some(usize::from(i))
    }

    /// Lists `slot` last on the battlefield.
    pub(crate) fn enter_battlefield(&mut self, slot: L::Slot) -> Result<(), Full> {
        debug_assert!(self.battlefield_position(slot).is_none());
        self.battlefield.push(slot)?;
        self.object_mut(slot).battlefield_index = (self.battlefield.len() - 1) as u16;
        Ok(())
    }

    /// Takes `slot` off the battlefield list, keeping the others in order.
    pub(crate) fn leave_battlefield(&mut self, slot: L::Slot) {
        let Some(i) = self.battlefield_position(slot) else {
            return;
        };
        self.battlefield.remove(i);
        self.object_mut(slot).battlefield_index = OFF_BATTLEFIELD;
        for j in i..self.battlefield.len() {
            let later = self.battlefield.as_slice()[j];
            self.object_mut(later).battlefield_index = j as u16;
        }
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise the base
    /// controller ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)).
    pub fn controller(&self, slot: L::Slot) -> Seat {
        let o = self.object(slot);
        if o.zone == Zone::Battlefield {
            self.derived(slot).controller
        } else {
            o.base_controller
        }
    }

    /// Who controls an object before continuous effects: for a permanent, the player who put
    /// it onto the battlefield, usually its owner
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)): "A permanent's owner
    /// is the same as the owner of the card that represents it ... A permanent's controller
    /// is, by default, the player under whose control it entered the battlefield."
    pub fn base_controller(&self, slot: L::Slot) -> Seat {
        self.object(slot).base_controller
    }

    /// The name of the object in `slot`: its face up's, which is the card's own name
    /// everywhere but on the stack and the battlefield
    /// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)): "While a
    /// double-faced card is outside the game or in a zone other than the battlefield or
    /// stack, it has only the characteristics of its front face." A face-down permanent,
    /// which has no name ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)),
    /// goes by its card's: what scenarios and saves call it.
    pub fn name(&self, t: &crate::table::GameTable, slot: L::Slot) -> &'static str {
        let def = t.def(self.card(slot));
        match self.object(slot).face {
            FaceIx::DOWN => def.name,
            face => def.face_name(face),
        }
    }

    /// The abilities waiting to be put on the stack.
    pub fn triggers(&self) -> &[PendingTrigger<L>] {
        self.triggers.as_slice()
    }

    /// The stack, bottom first.
    pub fn stack(&self) -> &[StackEntry<L>] {
        self.stack.as_slice()
    }

    /// The permissions to cast a card from a zone it could not otherwise be cast from.
    pub fn permissions(&self) -> &[CastPermission<L>] {
        self.permissions.as_slice()
    }

    /// The floating continuous effects.
    pub fn effects(&self) -> &[FloatingEffect<L>] {
        self.effects.as_slice()
    }

    /// Transitional: private with by-w8gv.5.3.
    pub fn object_mut(&mut self, slot: L::Slot) -> &mut Object<L> {
        &mut self.objects[slot.index()]
    }

    /// Transitional: private with by-w8gv.5.7.
    pub fn player_mut(&mut self, player: Seat) -> &mut Player<L> {
        &mut self.players[player.index()]
    }
}
