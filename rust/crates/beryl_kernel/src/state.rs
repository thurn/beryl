//! The game state, stored as its [`Layout`] says: in [`Small`], one flat `Copy` value with
//! fixed capacities and no heap (architecture §4, decisions D3 and D4).

pub use beryl_card::{CounterKind, TurnStep, Zone};

pub use crate::effects::{
    Affects, ContinuousEffect, CostChange, EFFECTS, Effects, LayerChange, LockedIn,
    MoveReplacement, Payload, Permission, Permitted, SETS,
};
pub use crate::memos::{MEMOS, Memo, Memos, MemosOf};
pub use crate::object::{Counters, Derived, GrantRef, Object, ObjectFlags, ObjectRef};
pub use crate::pending::{CastStep, Casting, Pending, Resolving};
pub use crate::player::{ManaPool, MulliganStatus, Player};
pub use crate::stack::{
    Lki, PendingTrigger, StackEntry, TARGETS, Target, TargetPlace, TargetPlaces, TriggerInfo,
    placed,
};
pub use crate::store::Duration;
pub use crate::turn::Turn;

use beryl_card::{FaceIx, Rules};

use crate::bitboard::Bitboard;
use crate::decision::Outcome;
use crate::fixed::Full;
use crate::ids::{Seat, Slot, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::object::OFF_BATTLEFIELD;
use crate::profile::{Limits, Profile};
use crate::rng::Rng;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = Slot::COUNT;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;

/// Names a zone, or a player's part of one, for [`State::zone`]
/// ([CR 400.1](https://yawgatog.com/resources/magic-rules/#R4001)): "Each player has their
/// own library, hand, and graveyard. The other zones are shared by all players."
///
/// Today the kernel keeps every zone but the stack by owner, so the battlefield and exile
/// are named a player's part at a time. Two zones the key can name and the kernel does not
/// yet keep:
///
/// - A zone shared by all players, as the rules see the battlefield and exile, is a variant
///   with no player, as [`ZoneKey::Stack`] is: add the variant, keep its membership once in
///   [`Zones`] and answer it in [`State::zone`].
/// - A command zone ([CR 408.1](https://yawgatog.com/resources/magic-rules/#R4081): "The
///   command zone is a game area reserved for certain specialized objects") is
///   `Of(player, Zone::Command)` for the objects a player owns there, once `beryl_card`'s
///   `Zone` has the variant and `Zone::OWNED` covers it, or a variant with no player if it
///   is kept shared.
///
/// Either way no caller's signature changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZoneKey {
    /// The objects `Seat` owns in a zone other than the stack: their library, hand or
    /// graveyard, or their cards on the battlefield or in exile.
    Of(Seat, Zone),
    /// The stack ([CR 405.1](https://yawgatog.com/resources/magic-rules/#R4051)).
    Stack,
}

/// Zone membership and the zones whose order the rules read (architecture §4.3). Only the
/// zone chokepoints write it ([`crate::zone`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Zones<L: Layout = Small> {
    /// Membership by owner, indexed by `Zone as usize` for the owned zones.
    pub(crate) members: L::PerPlayer<[Bitboard<L::Slot>; Zone::OWNED]>,
    pub(crate) stack: Bitboard<L::Slot>,
    /// Bottom first: the top card is the last.
    pub(crate) library: L::PerPlayer<L::Deck<L::Slot>>,
    /// Bottom first: the top card is the last.
    pub(crate) graveyard: L::PerPlayer<L::Deck<L::Slot>>,
    /// The next battlefield timestamp.
    pub(crate) next_timestamp: u16,
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
    /// A change since `derived` was last computed may have changed a layer result. Set by
    /// the mutators that change a layer input and cleared by [`State::rederive`] alone.
    pub(crate) derived_dirty: bool,
}

/// The whole game, stored as its [`Layout`] says: as [`Small`], copied with one `memcpy`
/// (architecture §4.1).
///
/// Every field is private to the crate (its contract: only the kernel writes a field).
///
/// The fields are laid out in the order written (`repr(C)`), which is chosen for the
/// rollout's instruction count: the small fields every decision reads come first, then the
/// zones, the objects and the layer results, then the queues, and last what milestone 4
/// (by-w8gv.7) changes, the stores and the queues they replace. A field's offset is part of
/// most loads of it, so a store that grows moves nothing before it. The whole order costs
/// [`Small`] 8 bytes of padding; in the order the compiler chose, the same fields counted
/// 0.9% more instructions (by-w8gv.7.2). A new field joins the group of its alignment, the
/// stores at the end. A hash of the state does not follow that order ([`State`]'s `Hash`).
#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct State<L: Layout = Small> {
    pub(crate) turn: Turn,
    pub(crate) players: L::PerPlayer<Player<L>>,
    pub(crate) budgets: Budgets,
    /// The kinds of event the permanents on the battlefield have triggered abilities for
    /// ([`crate::table::GameTable::listens`] of each), found with the layer results and
    /// current while they are.
    pub(crate) listening: u16,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) profile: Profile,
    pub(crate) flags: Flags,
    /// The rules the permanents' static abilities change for each player
    /// ([`beryl_card::Rule::whose`]), found with the layer results and current while they
    /// are: half of what [`State::rule_applies`] reads, the Effects store's own index
    /// being the other.
    pub(crate) static_rules: L::PerPlayer<Rules>,
    /// Slots on the battlefield, in the order they entered.
    pub(crate) battlefield: L::Queue<L::Slot, BATTLEFIELD>,
    pub(crate) rng: Rng,
    /// `knowledge[p]` holds the slots whose identity player `p` knows (architecture §13).
    pub(crate) knowledge: L::PerPlayer<Bitboard<L::Slot>>,
    /// The slots a store has an entry about as an object, so that an object with nothing
    /// stored about it ends at the cost of one bit test ([`crate::store`]).
    pub(crate) keyed: Bitboard<L::Slot>,
    pub(crate) zones: Zones<L>,
    /// The open question of a machine step: [`State::ask`] and its kin write it.
    pub(crate) pending: Pending<L>,
    /// Written through [`State::object_mut`] alone, which stays in the crate.
    pub(crate) objects: L::Table<Object<L>, SLOTS>,
    /// Layer results, by battlefield index.
    pub(crate) derived: L::Table<Derived<L>, BATTLEFIELD>,
    /// The stack's entries, bottom first: [`State::put_on_stack`] and its kin write it.
    pub(crate) stack: L::Queue<StackEntry<L>, 64>,
    /// The abilities that have triggered and wait to be put on the stack.
    pub(crate) triggers: L::Queue<PendingTrigger<L>, 64>,
    /// Delayed triggered abilities waiting for their moment (CR 603.7), which their
    /// ability's [`beryl_card::Trigger::Delayed`] names.
    pub(crate) delayed: L::Queue<PendingTrigger<L>, 32>,
    /// The Effects store ([`crate::effects`]): [`State::start_effect`] and
    /// [`State::expire`] write it.
    pub(crate) effects: Effects<L>,
    /// The Memos store ([`crate::memos`]): [`State::remember`] writes it, and the zone
    /// move ends what it holds about an object.
    pub(crate) memos: Memos<L>,
}

const _: () = assert!(size_of::<State<Small>>() <= 20 * 1024);

/// Every field in the order the game is described in, whatever order they are laid out in,
/// but the stores' index of keyed slots, which says nothing the stores do not: so a hash of
/// a state is a hash of the game, and moving a field for speed leaves it alone
/// (`beryl_bench` pins the hash of its corpus).
impl<L: Layout> std::hash::Hash for State<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let State {
            turn,
            players,
            budgets,
            listening,
            outcome,
            profile,
            flags,
            static_rules,
            battlefield,
            rng,
            knowledge,
            keyed: _,
            zones,
            pending,
            objects,
            derived,
            stack,
            triggers,
            delayed,
            effects,
            memos,
        } = self;
        objects.hash(h);
        battlefield.hash(h);
        derived.hash(h);
        listening.hash(h);
        static_rules.hash(h);
        zones.hash(h);
        players.hash(h);
        turn.hash(h);
        stack.hash(h);
        triggers.hash(h);
        delayed.hash(h);
        effects.hash(h);
        memos.hash(h);
        pending.hash(h);
        knowledge.hash(h);
        rng.hash(h);
        profile.hash(h);
        budgets.hash(h);
        flags.hash(h);
        outcome.hash(h);
    }
}

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

    /// The game index of the definition behind a slot: the kernel's own. Others ask a query
    /// ([`crate::query`]) or [`crate::Observation::card`].
    pub(crate) fn card(&self, slot: L::Slot) -> L::Card {
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

    #[inline]
    pub fn player(&self, player: Seat) -> &Player<L> {
        &self.players[player.index()]
    }

    #[inline]
    pub fn turn(&self) -> &Turn {
        &self.turn
    }

    #[inline]
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    #[inline]
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
    #[inline]
    pub fn rng(&self) -> Rng {
        self.rng
    }

    /// Reseeds the game's random streams, as search does for each copy it plays on so sibling
    /// rollouts diverge (architecture §14).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// The objects in the zone, or the player's part of one, that `key` names.
    #[inline]
    pub fn zone(&self, key: ZoneKey) -> Bitboard<L::Slot> {
        match key {
            ZoneKey::Of(owner, zone) => self.zones.members[owner.index()][zone as usize],
            ZoneKey::Stack => self.zones.stack,
        }
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

    #[inline]
    pub fn pending(&self) -> &Pending<L> {
        &self.pending
    }

    /// Whether `player` knows the identity of the object in `slot`.
    pub fn knows(&self, player: Seat, slot: L::Slot) -> bool {
        self.knowledge[player.index()].contains(slot)
    }

    /// The slots whose identity each player knows, by seat (architecture §13), to compare
    /// two positions by. A paused resolution keeps its own copy
    /// ([`State::pause_resolution`]).
    #[inline]
    pub fn knowledge(&self) -> L::PerPlayer<Bitboard<L::Slot>> {
        self.knowledge.clone()
    }

    /// Whether a change since the layer results were last stored may have changed one, so
    /// that [`State::derived`] must not be read before [`State::rederive`].
    #[inline]
    pub fn is_derived_stale(&self) -> bool {
        self.flags.derived_dirty
    }

    /// Every permanent with its characteristics after continuous effects, in the order they
    /// entered the battlefield.
    ///
    /// # Panics
    ///
    /// In debug builds, if the layer results are stale.
    #[inline]
    pub fn permanents(&self) -> impl Iterator<Item = (L::Slot, &Derived<L>)> + '_ {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        self.battlefield
            .iter()
            .copied()
            .zip(self.derived.as_slice().iter())
    }

    /// The kinds of event the permanents on the battlefield have triggered abilities for, as
    /// the layers last stored them, or nothing while the layer results are stale.
    #[inline]
    pub fn listening(&self) -> Option<u16> {
        (!self.flags.derived_dirty).then_some(self.listening)
    }

    /// A permanent's characteristics after continuous effects (architecture §9). Reads never
    /// recompute: every path that changes a layer input recomputes before anything reads.
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield, or, in debug builds, if `derived` is stale.
    pub fn derived(&self, slot: L::Slot) -> &Derived<L> {
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

    /// Who controls an object before continuous effects, which is what the layers start
    /// from and a saved position keeps: for a permanent, the player who put it onto the
    /// battlefield, usually its owner
    /// ([CR 110.2](https://yawgatog.com/resources/magic-rules/#R1102)): "A permanent's owner
    /// is the same as the owner of the card that represents it ... A permanent's controller
    /// is, by default, the player under whose control it entered the battlefield." Who
    /// controls it now is [`State::controller`].
    pub fn base_controller(&self, slot: L::Slot) -> Seat {
        self.object(slot).base_controller
    }

    /// The name of the object in `slot`: its face up's, which is the card's own name
    /// everywhere but on the stack and the battlefield
    /// ([CR 712.6a](https://yawgatog.com/resources/magic-rules/#R7126a)): "While a
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
    #[inline]
    pub fn triggers(&self) -> &[PendingTrigger<L>] {
        self.triggers.as_slice()
    }

    /// The stack, bottom first.
    #[inline]
    pub fn stack(&self) -> &[StackEntry<L>] {
        self.stack.as_slice()
    }

    /// The one way to an object to write, which never leaves the crate.
    #[inline]
    pub(crate) fn object_mut(&mut self, slot: L::Slot) -> &mut Object<L> {
        &mut self.objects[slot.index()]
    }
}
