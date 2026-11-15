//! The game state, stored as its [`Layout`] says: in [`Small`], one flat `Copy` value with
//! fixed capacities and no heap (architecture §4, decisions D3 and D4).

pub use beryl_card::{CounterKind, TurnStep, Zone};

pub use crate::counters::{
    COUNTERS, CounterRows, CounterTable, Counters, CountersHeld, CountersOf,
};
pub use crate::effects::{
    Affects, Characteristics, ChosenSource, Continuous, ContinuousEffect, CostChange, EFFECTS,
    Effects, EffectsOn, LayerChange, LockedIn, MoveReplacement, Payload, Permanents, Permission,
    Permitted, SETS, Shield, ShieldAt, Subject, read_layer,
};
pub use crate::last_known::{DEPARTED_SPELLS, DepartedSpell, LAST_KNOWN, LastKnown};
pub use crate::memos::{MEMOS, Memo, Memos, MemosOf};
pub use crate::object::{
    AbilityMask, AttackTarget, CombatRole, Derived, Flag, GrantRef, Object, ObjectFlags, ObjectRef,
    SlotSet,
};
pub use crate::pending::{CastKind, CastStep, Casting, PayingObjects, Pending, Resolving};
pub use crate::player::{ManaPool, MulliganStatus, Player};
pub use crate::stack::{
    AS_PRINTED, AbilityRef, AbilityTarget, PendingTrigger, READ_NOW, StackEntry, StackItem,
    TARGETS, Target, TargetPlace, TargetPlaces, TriggerInfo, TriggeredAbility, placed,
};
pub use crate::store::{Duration, Stored};
pub use crate::trackers::{Count, Tracked, Tracker, Trackers};
pub use crate::turn::Turn;

use beryl_card::FaceIx;

use crate::bitboard::Bitboard;
use crate::decision::Outcome;
use crate::event::{Listens, Replaces};
use crate::fixed::Full;
use crate::ids::{Seat, Slot, SlotId};
use crate::layout::{Layout, Queue, Small, Table};
use crate::object::OFF_BATTLEFIELD;
use crate::profile::{Limits, Profile};
use crate::repeats::Repeats;
use crate::rng::Rng;
pub use crate::rule_changes::{RuleId, RuleKind, Rules};
use crate::static_ability::Inputs;

/// Object slots: deck cards, tokens and spell copies.
pub const SLOTS: usize = Slot::COUNT;
/// Permanents on the battlefield at once.
pub const BATTLEFIELD: usize = 128;

/// Names a zone, or a player's part of one, for [`State::zone`] ([CR 400.1]): "Each player has
/// their own library, hand, and graveyard. The other zones are shared by all players."
///
/// Today the kernel keeps every zone but the stack by owner, so the battlefield and exile
/// are named a player's part at a time. Two zones the key can name and the kernel does not
/// yet keep:
///
/// - A zone shared by all players, as the rules see the battlefield and exile, is a variant
///   with no player, as [`ZoneKey::Stack`] is: add the variant, keep its membership once in
///   [`Zones`] and answer it in [`State::zone`].
/// - A command zone ([CR 408.1]: "The
///   command zone is a game area reserved for certain specialized objects") is
///   `Of(player, Zone::Command)` for the objects a player owns there, once `beryl_card`'s
///   `Zone` has the variant and `Zone::OWNED` covers it, or a variant with no player if it
///   is kept shared.
///
/// Either way no caller's signature changes.
///
/// [CR 400.1]: https://yawgatog.com/resources/magic-rules/#R4001
/// [CR 408.1]: https://yawgatog.com/resources/magic-rules/#R4081
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ZoneKey {
    /// The objects `Seat` owns in a zone other than the stack: their library, hand or
    /// graveyard, or their cards on the battlefield or in exile.
    Of(Seat, Zone),
    /// The stack ([CR 405.1]).
    ///
    /// [CR 405.1]: https://yawgatog.com/resources/magic-rules/#R4051
    Stack,
}

/// Zone membership and the zones whose order the rules read (architecture §4.3). Only the
/// zone chokepoints write it ([`crate::zone`]).
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// The fields in order, as a derived `Hash` would; a canonical hash reads the next timestamp
/// as the count of live ones ([`crate::repeats`]).
impl<L: Layout> std::hash::Hash for Zones<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Zones {
            members,
            stack,
            library,
            graveyard,
            next_timestamp,
        } = self;
        members.hash(h);
        stack.hash(h);
        library.hash(h);
        graveyard.hash(h);
        crate::repeats::next_timestamp(*next_timestamp).hash(h);
    }
}

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
    /// Some permanent has lost an ability of its face as the layers last applied: without
    /// one, [`State::has_ability`] answers yes with no read of the layer results.
    pub(crate) abilities_lost: bool,
    /// Some effect that removes abilities, or changes a land's land types and so the mana
    /// abilities they give ([CR 305.7]), applied as the layers last applied: without one,
    /// each permanent makes the mana its card's facts say, and the mana solver reads no layer
    /// result for it ([`State::makes_card_mana`]).
    ///
    /// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
    pub(crate) mana_changed: bool,
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
/// stores at the end. The Trackers store is the one store among the small fields, beside the
/// other sets of slots: every decision tests its bits for each loyalty ability it could
/// offer, and at the far end those tests counted 0.09% more (by-w8gv.7.8). A hash of the
/// state does not follow that order ([`State`]'s `Hash`).
#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub struct State<L: Layout = Small> {
    pub(crate) turn: Turn,
    pub(crate) players: L::PerPlayer<Player<L>>,
    pub(crate) budgets: Budgets,
    /// The kinds of event the permanents on the battlefield have triggered abilities for
    /// ([`crate::table::GameTable::listens`] of each), found with the layer results and
    /// current while they are.
    pub(crate) listening: Listens,
    /// The kinds of event the static abilities of the permanents on the battlefield replace,
    /// and whether one changes a rule for players ([`crate::table::GameTable::replaces`] of
    /// each), found with the layer results and current while they are.
    pub(crate) replacing: Replaces,
    pub(crate) outcome: Option<Outcome>,
    pub(crate) profile: Profile,
    pub(crate) flags: Flags,
    /// What the static abilities of the permanents on the battlefield read that no layer
    /// writes ([`Inputs`]), found with the layer results: a mutator that changes one of
    /// these marks the results stale.
    pub(crate) layer_inputs: Inputs,
    /// The rules the permanents' static abilities change for each player
    /// ([`GameTable::player_rules`]), found with the layer results and current while they
    /// are: half of what [`State::rule_applies`] reads, the Effects store's own index
    /// being the other.
    pub(crate) static_rules: L::PerPlayer<Rules>,
    /// Slots on the battlefield, in the order they entered.
    pub(crate) battlefield: L::Queue<L::Slot, BATTLEFIELD>,
    pub(crate) rng: Rng,
    /// The resolutions in a row with no choice, and the hash a loop of mandatory actions is
    /// compared with ([`crate::repeats`]).
    pub(crate) repeats: Repeats,
    /// `knowledge[p]` holds the slots whose identity player `p` knows (architecture §13).
    pub(crate) knowledge: L::PerPlayer<Bitboard<L::Slot>>,
    /// The slots a store has an entry about as an object, so that an object with nothing
    /// stored about it ends at the cost of one bit test ([`crate::store`]).
    pub(crate) keyed: Bitboard<L::Slot>,
    /// The Trackers store ([`crate::trackers`]): [`State::track`] writes it, the turn's
    /// beginning empties it and the zone move ends what it holds about a permanent. Here
    /// and not with the other stores for the decision loop's reads of it.
    pub(crate) trackers: Trackers<L>,
    pub(crate) zones: Zones<L>,
    /// The open question of a machine step: [`State::ask`] and its kin write it.
    pub(crate) pending: Pending<L>,
    /// Written through [`State::object_mut`] alone, which stays in the crate.
    pub(crate) objects: L::Table<Object<L>, SLOTS>,
    /// Layer results, by battlefield index.
    pub(crate) derived: L::Table<Derived<L>, BATTLEFIELD>,
    /// The stack's entries, bottom first: [`State::put_on_stack`] and its kin write it.
    pub(crate) stack: L::Queue<StackEntry<L>, 64>,
    /// The text changes of the sources of abilities on the stack, as each went on it, which
    /// a [`StackEntry::words`] names ([`crate::effects::text`]). Emptied with the stack.
    pub(crate) captured_words: L::Queue<beryl_card::Words, CAPTURED_WORDS>,
    /// The abilities that have triggered and wait to be put on the stack.
    pub(crate) triggers: L::Queue<PendingTrigger<L>, 64>,
    /// Delayed triggered abilities waiting for their moment (CR 603.7), which their
    /// ability's [`beryl_card::Trigger`], a delayed one, names.
    pub(crate) delayed: L::Queue<PendingTrigger<L>, 32>,
    /// The last-known information of permanents that left the battlefield, which abilities
    /// that look back read ([`crate::last_known`]): [`State::record_last_known`] writes it
    /// and [`State::forget_last_known`] empties it.
    pub(crate) last_known: L::Queue<LastKnown<L>, LAST_KNOWN>,
    /// What spells that left the stack without resolving were there, for abilities of their
    /// own that copy them ([`crate::last_known::DepartedSpell`]): written as a spell leaves
    /// ([`State::take_off_stack`]) and emptied with the last-known information. Empty in a
    /// game whose cards copy no spell of their own.
    pub(crate) departed_spells: L::Queue<DepartedSpell<L>, DEPARTED_SPELLS>,
    /// The Effects store ([`crate::effects`]): [`State::start_effect`] and
    /// [`State::expire`] write it.
    pub(crate) effects: Effects<L>,
    /// The Memos store ([`crate::memos`]): [`State::remember`] writes it, and the zone
    /// move ends what it holds about an object.
    pub(crate) memos: Memos<L>,
    /// The table of the counters no object keeps in a field ([`crate::counters`]):
    /// [`State::add_counters`] and [`State::remove_counters`] write it, and the zone move
    /// ends an object's rows.
    pub(crate) counters: CounterTable<L>,
    /// The continuous effects that reach objects off the battlefield, in the order they
    /// apply, as the layers last applied ([`crate::elsewhere`]): stored with the layer
    /// results, and empty in most games.
    pub(crate) elsewhere: crate::elsewhere::Reaching<L>,
}

const _: () = assert!(size_of::<State<Small>>() <= 20 * 1024);

/// The most sets of text changes the abilities on the stack keep in a [`Small`] game. One past
/// it stalls the game into [`crate::Large`].
pub const CAPTURED_WORDS: usize = 4;

/// Every field in the order the game is described in, whatever order they are laid out in,
/// but the stores' index of keyed slots, which says nothing the stores do not: so a hash of
/// a state is a hash of the game, and moving a field for speed leaves it alone
/// (`beryl_bench` pins the hash of its corpus).
impl<L: Layout> std::hash::Hash for State<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.hash_game(h, true);
    }
}

impl<L: Layout> State<L> {
    /// [`State`]'s `Hash`, with the budgets or without them ([`State::canonical_hash`]).
    pub(crate) fn hash_game<H: std::hash::Hasher>(&self, h: &mut H, with_budgets: bool) {
        use std::hash::Hash;
        let State {
            turn,
            players,
            budgets,
            listening,
            replacing,
            // Found from the faces on the battlefield, which the hash covers.
            layer_inputs: _,
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
            captured_words,
            triggers,
            delayed,
            last_known,
            departed_spells,
            effects,
            memos,
            trackers,
            counters,
            // Found from the faces on the battlefield and the memos, which the hash covers.
            elsewhere: _,
            // Bookkeeping about the past that only the loop check reads ([`crate::repeats`]).
            repeats: _,
        } = self;
        objects.hash(h);
        battlefield.hash(h);
        derived.hash(h);
        listening.hash(h);
        replacing.hash(h);
        static_rules.hash(h);
        zones.hash(h);
        players.hash(h);
        turn.hash(h);
        stack.hash(h);
        // Empty in a game without a text change, which hashes as it did before the table.
        if !captured_words.is_empty() {
            captured_words.hash(h);
        }
        triggers.hash(h);
        delayed.hash(h);
        last_known.hash(h);
        // Empty in a game whose cards copy no spell of their own, which hashes as it did
        // before the table.
        if !departed_spells.is_empty() {
            departed_spells.hash(h);
        }
        effects.hash(h);
        memos.hash(h);
        trackers.hash(h);
        counters.hash(h);
        pending.hash(h);
        knowledge.hash(h);
        rng.hash(h);
        profile.hash(h);
        if with_budgets {
            budgets.hash(h);
        }
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

    /// Whether `player` knows the identity of the object in `slot`. A player controlling the
    /// active player this turn knows what that player knows, which they learn as that player
    /// does ([`State::sharer`]).
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

    /// The layer results, one for each permanent, in battlefield order.
    ///
    /// # Panics
    ///
    /// In debug builds, if the layer results are stale.
    #[inline]
    pub fn layer_results(&self) -> &[Derived<L>] {
        debug_assert!(!self.flags.derived_dirty, "derived read while dirty");
        &self.derived.as_slice()[..self.battlefield.len()]
    }

    /// The kinds of event the permanents on the battlefield have triggered abilities for, as
    /// the layers last stored them, or nothing while the layer results are stale.
    #[inline]
    pub fn listening(&self) -> Option<Listens> {
        (!self.flags.derived_dirty).then_some(self.listening)
    }

    /// The kinds of event the static abilities of the permanents on the battlefield replace,
    /// and whether one changes a rule for players, as the layers last stored them, or nothing
    /// while the layer results are stale.
    #[inline]
    pub fn replacing(&self) -> Option<Replaces> {
        (!self.flags.derived_dirty).then_some(self.replacing)
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

    /// Who controls an object before continuous effects, which is what the layers start from and a
    /// saved position keeps: for a permanent, the player who put it onto the battlefield, usually
    /// its owner ([CR 110.2]): "A permanent's owner is the same as the owner of the card that
    /// represents it ... A permanent's controller is, by default, the player under whose control it
    /// entered the battlefield." Who controls it now is [`State::controller`].
    ///
    /// [CR 110.2]: https://yawgatog.com/resources/magic-rules/#R1102
    pub fn base_controller(&self, slot: L::Slot) -> Seat {
        self.object(slot).base_controller
    }

    /// The name of the object in `slot`: its face up's, which is the card's own name everywhere but
    /// on the stack and the battlefield ([CR 712.6a]): "While a double-faced card is outside the
    /// game or in a zone other than the battlefield or stack, it has only the characteristics of
    /// its front face." A face-down permanent, which has no name ([CR 708.2]), goes by its card's:
    /// what scenarios and saves call it.
    ///
    /// [CR 712.6a]: https://yawgatog.com/resources/magic-rules/#R7126a
    /// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
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
