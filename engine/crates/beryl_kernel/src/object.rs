//! Objects: the base data of each slot, a reference to one incarnation of it, and the layer
//! results of a permanent (architecture §4.2, §5, §9).
//!
//! Implements: none. Holds the fields of an object and exposes their stored values.
//!
//! Only the kernel writes an object: through [`State`](crate::State)'s mutators, the zone and
//! chance chokepoints and the board door. No `&mut Object` leaves the
//! crate. Other crates read an object through the getters below.

use std::fmt::Debug;
use std::hash::Hash;

use beryl_card::{AbilityIx, Colors, FaceIx, GrantIx, Keywords, Supertypes, Types, Zone};

use crate::counters::Counters;
use crate::ids::{CardId, CardIx, Seat, Slot, SlotId};
use crate::layout::{Layout, Small};

mod block_list;
pub use block_list::{BlockList, SlotSet};

/// The [`Object::battlefield_index`] of a slot not on the battlefield.
pub(crate) const OFF_BATTLEFIELD: u16 = u16::MAX;

/// A status an object keeps in its [`ObjectFlags`]. Each variant's value is its bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Flag {
    /// see [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105): "A permanent's status is its
    /// physical state. There are four status categories, each of which has two possible values:
    /// tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased out."
    Tapped = 1 << 0,
    /// Not under its controller's control continuously since their most recent turn began
    /// (see [CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    SummoningSick = 1 << 1,
    /// Phased out (see [CR 702.26b]): "If a permanent phases out, its status changes to “phased
    /// out.” Except for rules and effects that specifically mention phased-out permanents, a
    /// phased-out permanent is treated as though it does not exist." Its slot is in the state's
    /// set of phased-out permanents ([`crate::phasing`]) and in no battlefield list.
    ///
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    PhasedOut = 1 << 2,
    /// A token (see [CR 111.1]): "a marker used to represent any permanent that isn't
    /// represented by a card."
    ///
    /// [CR 111.1]: https://yawgatog.com/resources/magic-rules/#R1111
    Token = 1 << 3,
    /// A copy effect is on the object, an entry of the Effects store
    /// ([`crate::effects::Continuous::Copy`]): the object's definition and face are the values
    /// it copied, and the entry holds the exceptions it was made with (see [CR 707.9b]): "Some copy
    /// effects modify a characteristic as part of the copying process. The final set of values
    /// for that characteristic becomes part of the copiable values of the copy." An object
    /// without it has no copy effect to look for, so reading its copiable values is one bit
    /// test ([`State::copiable`](crate::State::copiable)).
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    Copy = 1 << 4,
    /// Flipped, a status independent of which side of a card is face up (see CR 110.5).
    Flipped = 1 << 7,
}

impl Flag {
    const fn bit(self) -> u8 {
        self as u8
    }
}

/// What an Aura, Equipment, or Fortification is attached to (see [CR 701.3a]).
///
/// [CR 701.3a]: https://yawgatog.com/resources/magic-rules/#R7013a
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Attachment<S> {
    Object(S),
    Player(Seat),
}

/// An object's statuses, one bit per [`Flag`]. Combat is no flag: it is the object's
/// [`CombatRole`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectFlags(u8);

const _: () = assert!(Flag::Flipped.bit() & ObjectFlags::PLAYER_MASK == 0);

impl ObjectFlags {
    pub(crate) const NONE: ObjectFlags = ObjectFlags(0);

    // The seat plus one occupies two spare bits. Zero means no player, and taking
    // the attachment off the battlefield clears both bits with the other flags.
    const PLAYER_SHIFT: u32 = 5;
    const PLAYER_MASK: u8 = 0b11 << Self::PLAYER_SHIFT;

    fn attached_player(self) -> Option<Seat> {
        let seat = (self.0 & Self::PLAYER_MASK) >> Self::PLAYER_SHIFT;
        seat.checked_sub(1).and_then(Seat::try_new)
    }

    fn set_attached_player(&mut self, player: Option<Seat>) {
        let seat = player.map_or(0, |player| player.get() + 1);
        debug_assert!(seat <= Self::PLAYER_MASK >> Self::PLAYER_SHIFT);
        self.0 = (self.0 & !Self::PLAYER_MASK) | (seat << Self::PLAYER_SHIFT);
    }

    #[inline]
    pub fn has(self, flag: Flag) -> bool {
        self.0 & flag.bit() != 0
    }

    #[inline]
    pub(crate) fn set(&mut self, flag: Flag, on: bool) {
        if on {
            self.0 |= flag.bit();
        } else {
            self.0 &= !(flag.bit());
        }
    }

    /// The flags an object keeps as it changes zones: being a token, and a token's copy
    /// effect, which the zone move carries over ([`State::move_object`]). A token stays a token
    /// wherever it goes, until it ceases to exist (see [CR 111.7]).
    ///
    /// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
    /// [`State::move_object`]: crate::State::move_object
    #[inline]
    pub(crate) fn kept_across_zones(self) -> ObjectFlags {
        ObjectFlags(self.0 & (Flag::Token.bit() | Flag::Copy.bit()))
    }
}

/// What an attacking creature attacks (see [CR 506.2]): "that player, planeswalkers they
/// control, and battles they protect may be attacked."
///
/// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttackTarget<L: Layout = Small> {
    Player(Seat),
    Planeswalker(L::Slot),
    /// The planeswalker it attacked was removed from combat (see [CR 506.4c]): "It continues
    /// to be an attacking creature, although it is not attacking any player, planeswalker,
    /// or battle."
    ///
    /// [CR 506.4c]: https://yawgatog.com/resources/magic-rules/#R5064c
    Removed,
}

/// What a permanent is doing in combat (see [CR 506.3]): "Only a creature can attack or
/// block." A creature is attacking, blocking or neither, never both.
///
/// [CR 506.3]: https://yawgatog.com/resources/magic-rules/#R5063
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CombatRole<L: Layout = Small> {
    #[default]
    None,
    /// An attacking creature (see [CR 508.1k]): "Each chosen creature still controlled by the
    /// active player becomes an attacking creature."
    ///
    /// [CR 508.1k]: https://yawgatog.com/resources/magic-rules/#R5081k
    Attacking {
        target: AttackTarget<L>,
        /// Blocked (see [CR 509.1h]): "An attacking creature with one or more creatures
        /// declared as blockers for it becomes a blocked creature ... A creature remains
        /// blocked even if all the creatures blocking it are removed from combat."
        ///
        /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
        blocked: bool,
        /// Had first strike or double strike when the first damage step began (see [CR 510.4]).
        ///
        /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
        struck_first: bool,
    },
    /// A blocking creature (see [CR 509.1g]): "Each chosen creature still controlled by the
    /// defending player becomes a blocking creature." It blocks no attacker between its
    /// declaration and the choice of what it blocks, and none once those attackers have
    /// left combat (see [CR 506.4]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    Blocking {
        attackers: SlotSet<L>,
        /// Had first strike or double strike when the first damage step began (see [CR 510.4]).
        ///
        /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
        struck_first: bool,
    },
}

impl<L: Layout> Copy for CombatRole<L> where L::BlockList: Copy {}

impl<L: Layout> CombatRole<L> {
    /// An attacking creature that attacks `target`, not yet blocked.
    #[inline]
    pub(crate) fn attacking(target: AttackTarget<L>) -> CombatRole<L> {
        CombatRole::Attacking {
            target,
            blocked: false,
            struck_first: false,
        }
    }

    /// The slots it names: the planeswalker it attacks, or the attackers it blocks.
    #[inline]
    pub(crate) fn names(&self) -> impl Iterator<Item = L::Slot> + '_ {
        let (walker, attackers) = match self {
            CombatRole::Attacking {
                target: AttackTarget::Planeswalker(walker),
                ..
            } => (Some(*walker), None),
            CombatRole::Blocking { attackers, .. } => (None, Some(attackers.iter())),
            CombatRole::None | CombatRole::Attacking { .. } => (None, None),
        };
        walker.into_iter().chain(attackers.into_iter().flatten())
    }

    /// Stops naming `slot`, which has left combat (see [CR 506.4]): "A planeswalker or battle
    /// that's removed from combat stops being attacked." What attacked it or blocked it
    /// stays in combat, related to nothing.
    ///
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    #[inline]
    pub(crate) fn forget(&mut self, slot: L::Slot) {
        match self {
            CombatRole::Attacking { target, .. } if *target == AttackTarget::Planeswalker(slot) => {
                *target = AttackTarget::Removed;
            }
            CombatRole::Blocking { attackers, .. } => attackers.remove(slot),
            CombatRole::None | CombatRole::Attacking { .. } => {}
        }
    }

    /// The same role with its slots renumbered by `f`.
    #[inline]
    pub(crate) fn map<M: Layout>(&self, f: impl Fn(L::Slot) -> M::Slot) -> CombatRole<M> {
        match self {
            CombatRole::None => CombatRole::None,
            CombatRole::Attacking {
                target,
                blocked,
                struck_first,
            } => CombatRole::Attacking {
                target: match target {
                    AttackTarget::Player(p) => AttackTarget::Player(*p),
                    AttackTarget::Planeswalker(w) => AttackTarget::Planeswalker(f(*w)),
                    AttackTarget::Removed => AttackTarget::Removed,
                },
                blocked: *blocked,
                struck_first: *struck_first,
            },
            CombatRole::Blocking {
                attackers,
                struck_first,
            } => CombatRole::Blocking {
                attackers: attackers.map::<M>(f),
                struck_first: *struck_first,
            },
        }
    }

    /// The role as the bits and the one slot the object's hash has always read, so that
    /// the hash of a game stays what it was before combat had a type of its own: the
    /// rollout and corpus pins in `beryl_bench` hash states.
    #[inline]
    fn legacy(&self) -> (u32, Option<L::Slot>) {
        const ATTACKING: u32 = 1 << 2;
        const BLOCKING: u32 = 1 << 8;
        const BLOCKED: u32 = 1 << 9;
        const ATTACKS_PLANESWALKER: u32 = 1 << 10;
        const STRUCK_FIRST: u32 = 1 << 11;
        let first = |on: bool| if on { STRUCK_FIRST } else { 0 };
        match self {
            CombatRole::None => (0, None),
            CombatRole::Attacking {
                target,
                blocked,
                struck_first,
            } => {
                let blocked = if *blocked { BLOCKED } else { 0 };
                let (walker, slot) = match target {
                    AttackTarget::Player(_) => (0, None),
                    AttackTarget::Planeswalker(w) => (ATTACKS_PLANESWALKER, Some(*w)),
                    AttackTarget::Removed => (ATTACKS_PLANESWALKER, None),
                };
                (ATTACKING | blocked | walker | first(*struck_first), slot)
            }
            CombatRole::Blocking {
                attackers,
                struck_first,
            } => (BLOCKING | first(*struck_first), attackers.first()),
        }
    }
}

/// The base data of the object in one slot (architecture §4.2).
///
/// The fields are `pub(crate)`: other crates read an object through its getters, and only
/// the kernel can write one, since no `&mut Object` leaves the crate.
///
/// Aligned to 2 bytes, so a `Small` object is 22 bytes, its fields with no padding. It was
/// once aligned to 4 bytes and 24 bytes long, when the rollout benchmark counted about
/// 118,000 fewer instructions indexing a table of 24-byte objects; measured again with the
/// engine as it is now, 22-byte objects count 0.01% fewer, and the 255 slots take 510 bytes
/// fewer of a [`State<Small>`](crate::state::State).
#[derive(Debug, PartialEq, Eq)]
#[repr(align(2))]
pub struct Object<L: Layout = Small> {
    /// Index into the game table: the definition its values are read from. The kernel's
    /// own: others ask a query ([`crate::query`]) or [`crate::Observation::card`].
    pub(crate) card: L::Card,
    /// Bumped on every zone change (see CR 400.7), so stale references are detectable.
    pub(crate) incarnation: u16,
    pub(crate) zone: Zone,
    pub(crate) owner: Seat,
    /// The controller before continuous effects: the owner, but for a spell and a permanent
    /// ([`State::controller`](crate::State::controller),
    /// [`State::base_controller`](crate::State::base_controller)).
    pub(crate) base_controller: Seat,
    pub(crate) flags: ObjectFlags,
    /// Damage marked this turn.
    pub(crate) damage: u8,
    pub(crate) counters: Counters,
    /// The object an Aura or Equipment is attached to.
    pub(crate) attached_to: Option<L::Slot>,
    /// Battlefield entry order, for layers.
    pub(crate) timestamp: u16,
    /// Which face is up.
    pub(crate) face: FaceIx,
    /// What it is doing in combat.
    pub(crate) combat: CombatRole<L>,
    /// The object's index in [`State::battlefield`], so finding a permanent's `derived` is
    /// one read rather than a search of the battlefield; [`OFF_BATTLEFIELD`] off it. Only
    /// [`State::enter_battlefield`] and [`State::leave_battlefield`] change it, and it fills
    /// what was padding.
    pub(crate) battlefield_index: u16,
}

// A small object's clone is a copy, as with State<Small>.
#[allow(clippy::expl_impl_clone_on_copy)]
impl<L: Layout> Clone for Object<L> {
    #[inline]
    fn clone(&self) -> Self {
        L::clone_object(self)
    }
}

impl Copy for Object<Small> {}

impl<L: Layout> Object<L> {
    pub const EMPTY: Object<L> = Object {
        card: L::Card::FIRST,
        incarnation: 0,
        zone: Zone::Nowhere,
        owner: Seat::P0,
        base_controller: Seat::P0,
        flags: ObjectFlags::NONE,
        damage: 0,
        counters: Counters::NONE,
        attached_to: None,
        timestamp: 0,
        face: FaceIx::FRONT,
        combat: CombatRole::None,
        battlefield_index: OFF_BATTLEFIELD,
    };
}

/// What the rules give an object, to read. A field has one getter, and a status flag one
/// each, but its definition's index and its base controller: those are answered by
/// [`State`](crate::State)'s queries ([`crate::query`]).
impl<L: Layout> Object<L> {
    /// Bumped on every zone change (see [CR 400.7]): "An object that moves from one zone to another
    /// becomes a new object".
    ///
    /// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
    #[inline]
    pub fn incarnation(&self) -> u16 {
        self.incarnation
    }

    #[inline]
    pub fn zone(&self) -> Zone {
        self.zone
    }

    /// On the battlefield and phased in: a permanent the game treats as existing, with layer
    /// results (see [CR 702.26b]): "a phased-out permanent is treated as though it does not exist."
    /// A phased-out permanent's [`Object::zone`] is still the battlefield (see [CR 702.26d]): "The
    /// phasing event doesn't actually cause a permanent to change zones". As cheap as a test of
    /// the zone: one read of the battlefield index.
    ///
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    /// [CR 702.26d]: https://yawgatog.com/resources/magic-rules/#R70226d
    #[inline]
    pub fn on_battlefield(&self) -> bool {
        self.battlefield_index != OFF_BATTLEFIELD
    }

    /// see [CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083): "The owner of a card in the
    /// game is the player who started the game with it in their deck."
    #[inline]
    pub fn owner(&self) -> Seat {
        self.owner
    }

    /// Every status flag at once.
    #[inline]
    pub fn flags(&self) -> ObjectFlags {
        self.flags
    }

    /// see [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105): "A permanent's status is its
    /// physical state. There are four status categories, each of which has two possible values:
    /// tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased out."
    #[inline]
    pub fn is_tapped(&self) -> bool {
        self.flags.has(Flag::Tapped)
    }

    /// Not under its controller's control continuously since their most recent turn began
    /// (see [CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    #[inline]
    pub fn is_summoning_sick(&self) -> bool {
        self.flags.has(Flag::SummoningSick)
    }

    /// Phased out ([`Flag::PhasedOut`]): on the battlefield by its zone, and treated as though
    /// it does not exist.
    #[inline]
    pub fn is_phased_out(&self) -> bool {
        self.flags.has(Flag::PhasedOut)
    }

    /// see [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "a marker used to represent
    /// any permanent that isn't represented by a card."
    #[inline]
    pub fn is_token(&self) -> bool {
        self.flags.has(Flag::Token)
    }

    /// What it is doing in combat.
    #[inline]
    pub fn combat(&self) -> &CombatRole<L> {
        &self.combat
    }

    /// An attacking creature (see [CR 506.3]).
    ///
    /// [CR 506.3]: https://yawgatog.com/resources/magic-rules/#R5063
    #[inline]
    pub fn is_attacking(&self) -> bool {
        matches!(self.combat, CombatRole::Attacking { .. })
    }

    /// A blocking creature (see [CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn is_blocking(&self) -> bool {
        matches!(self.combat, CombatRole::Blocking { .. })
    }

    /// An attacking creature that has been blocked (see [CR 509.1h]).
    ///
    /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
    #[inline]
    pub fn is_blocked(&self) -> bool {
        matches!(self.combat, CombatRole::Attacking { blocked: true, .. })
    }

    /// What an attacking creature attacks (see [CR 506.2]); `None` if it is not attacking.
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    #[inline]
    pub fn attack_target(&self) -> Option<AttackTarget<L>> {
        match self.combat {
            CombatRole::Attacking { target, .. } => Some(target),
            CombatRole::None | CombatRole::Blocking { .. } => None,
        }
    }

    /// The planeswalker an attacking creature attacks (see [CR 506.2]).
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    #[inline]
    pub fn attacked_planeswalker(&self) -> Option<L::Slot> {
        match self.combat {
            CombatRole::Attacking {
                target: AttackTarget::Planeswalker(walker),
                ..
            } => Some(walker),
            CombatRole::None | CombatRole::Attacking { .. } | CombatRole::Blocking { .. } => None,
        }
    }

    /// Whether it is a blocking creature that blocks `attacker` (see [CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn blocks(&self, attacker: L::Slot) -> bool {
        matches!(&self.combat, CombatRole::Blocking { attackers, .. } if attackers.contains(attacker))
    }

    /// The attackers a blocking creature blocks, none for any other object (see [CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn blocked_attackers(&self) -> SlotSet<L> {
        match &self.combat {
            CombatRole::Blocking { attackers, .. } => attackers.clone(),
            CombatRole::None | CombatRole::Attacking { .. } => SlotSet::EMPTY,
        }
    }

    /// Had first strike or double strike when the first damage step began (see [CR 510.4]).
    ///
    /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
    #[inline]
    pub fn struck_first(&self) -> bool {
        matches!(
            self.combat,
            CombatRole::Attacking {
                struck_first: true,
                ..
            } | CombatRole::Blocking {
                struck_first: true,
                ..
            }
        )
    }

    /// Damage marked this turn (see [CR 120.6]): "Damage marked on a creature remains until the cleanup
    /// step".
    ///
    /// [CR 120.6]: https://yawgatog.com/resources/magic-rules/#R1206
    #[inline]
    pub fn damage(&self) -> u8 {
        self.damage
    }

    /// Its counters of the kinds the rules themselves read; of any kind,
    /// [`State::counters_of`](crate::State::counters_of).
    #[inline]
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// The object an Aura or Equipment is attached to (see [CR 701.3]).
    ///
    /// [CR 701.3]: https://yawgatog.com/resources/magic-rules/#R7013
    #[inline]
    pub fn attached_to(&self) -> Option<L::Slot> {
        self.attached_to
    }

    /// The player this object enchants (see [CR 303.4b]).
    ///
    /// [CR 303.4b]: https://yawgatog.com/resources/magic-rules/#R3034b
    pub fn enchanted_player(&self) -> Option<Seat> {
        self.flags.attached_player()
    }

    /// The object or player this object is attached to.
    pub fn attachment(&self) -> Option<Attachment<L::Slot>> {
        self.attached_to()
            .map(Attachment::Object)
            .or_else(|| self.enchanted_player().map(Attachment::Player))
    }

    pub(crate) fn set_attachment(&mut self, host: Option<Attachment<L::Slot>>) {
        match host {
            Some(Attachment::Object(slot)) => {
                self.attached_to = Some(slot);
                self.flags.set_attached_player(None);
            }
            Some(Attachment::Player(player)) => {
                self.attached_to = None;
                self.flags.set_attached_player(Some(player));
            }
            None => {
                self.attached_to = None;
                self.flags.set_attached_player(None);
            }
        }
    }

    /// When it entered the battlefield or last turned face up, among the layers' timestamps
    /// (see [CR 613.7]).
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    #[inline]
    pub fn timestamp(&self) -> u16 {
        self.timestamp
    }

    /// Which face is up (see [CR 712.6]), or [`FaceIx::DOWN`] (see [CR 708.2]).
    ///
    /// [CR 712.6]: https://yawgatog.com/resources/magic-rules/#R7126
    /// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
    #[inline]
    pub fn face(&self) -> FaceIx {
        self.face
    }

    /// The slots it names ([`crate::State::named`]): what it is attached to, and what it
    /// attacks or blocks.
    #[inline]
    pub(crate) fn names(&self) -> impl Iterator<Item = L::Slot> + '_ {
        self.attached_to.into_iter().chain(self.combat.names())
    }
}

const _: () = assert!(size_of::<Option<Slot>>() == 1 && size_of::<Option<CardIx>>() == 1);
const _: () = assert!(size_of::<Object<Small>>() == 22);

/// Every field but the battlefield index, a cache of the battlefield list's order, in
/// declaration order, as a derived `Hash` would: so a hash of a state is a hash of the game.
impl<L: Layout> std::hash::Hash for Object<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Object {
            card,
            incarnation,
            zone,
            owner,
            base_controller,
            flags,
            damage,
            counters,
            attached_to,
            timestamp,
            face,
            combat,
            battlefield_index: _,
        } = self;
        card.hash(h);
        crate::repeats::incarnation(*incarnation).hash(h);
        zone.hash(h);
        owner.hash(h);
        base_controller.hash(h);
        // The flags and combat role hash as the one `u32` of bits and the one slot they
        // were before combat had a type of its own ([`CombatRole::legacy`]).
        let (combat_bits, combat_slot) = combat.legacy();
        (u32::from(flags.0) | combat_bits).hash(h);
        damage.hash(h);
        counters.hash(h);
        attached_to.hash(h);
        crate::repeats::timestamp(*timestamp, crate::repeats::timestamp_live(*zone)).hash(h);
        face.hash(h);
        combat_slot.hash(h);
        if let CombatRole::Blocking { attackers, .. } = combat
            && attackers.is_multiple()
        {
            attackers.hash(h);
        }
    }
}

/// A reference to one incarnation of the object in a slot (architecture §5). A reference
/// whose incarnation no longer matches names an object that has left its zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectRef<L: Layout = Small> {
    pub slot: L::Slot,
    pub incarnation: u16,
}

/// The slot and the incarnation, as a derived `Hash` would; a canonical hash reads the
/// incarnation as how long ago it was current ([`crate::repeats`]).
impl<L: Layout> std::hash::Hash for ObjectRef<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.slot.hash(h);
        crate::repeats::reference(self.slot.index(), self.incarnation).hash(h);
    }
}

/// The result of applying continuous effects to a permanent (architecture §9), by
/// battlefield index. The layers work fills it in.
///
/// Packed to an alignment of two bytes: its fields fill 40 bytes, and with the two of
/// [`Derived::lost`] an alignment of eight would round each of the 128 rows up to 48, which
/// the 20 KiB of a [`State<Small>`](crate::state::State) has no room for. Fields are read and
/// written by value; a reference to one does not compile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(Rust, packed(2))]
pub struct Derived<L: Layout = Small> {
    pub power: i8,
    pub toughness: i8,
    pub types: Types,
    pub supertypes: Supertypes,
    pub colors: Colors,
    pub controller: Seat,
    /// The colors it has protection from.
    pub protection: Colors,
    pub keywords: Keywords,
    /// By the game's numbers: read them through [`crate::GameTable::subtypes`].
    pub subtypes: crate::subtypes::GameSubtypes,
    /// The abilities other objects grant this permanent (§9), in the order granted.
    pub granted: [Option<GrantRef<L>>; 4],
    /// The abilities of its face it has lost, by their index on the face
    /// (see [CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing
    /// effects, and effects that say an object can't have an ability are applied." Read it
    /// through [`crate::state::State::has_ability`], which every reader asks.
    ///
    /// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
    pub lost: L::AbilityMask,
}

/// The abilities of a face a permanent has lost, one bit for each, by the index of the
/// ability on the face: as many as the layout's mask is wide ([`AbilityMask::WIDTH`]). An
/// effect that would remove an ability past that width, or all the abilities of a face
/// with more, has no place in the mask, and the recompute stalls the game into a layout
/// whose mask is wider.
pub trait AbilityMask: Copy + Debug + Default + Eq + Hash + Send + Sync + 'static {
    /// How many abilities of a face the mask can name.
    const WIDTH: usize;
    /// Every ability the mask can name: "loses all abilities".
    const ALL: Self;
    /// Whether the ability at `ability` is lost.
    fn contains(self, ability: AbilityIx) -> bool;
    /// The mask with the ability at `ability` lost too, or `None` past the mask's width.
    fn with(self, ability: AbilityIx) -> Option<Self>;
    /// Whether no ability is lost.
    fn is_empty(self) -> bool;
    /// The mask as bits, the ability at index `n` in bit `n`.
    fn bits(self) -> u64;
    /// The mask of `bits`, the ability at index `n` in bit `n`, dropping the bits past the
    /// mask's width.
    fn from_bits(bits: u64) -> Self;
}

macro_rules! ability_mask {
    ($($bits:ty),*) => {$(
        impl AbilityMask for $bits {
            const WIDTH: usize = <$bits>::BITS as usize;
            const ALL: Self = <$bits>::MAX;

            #[inline]
            fn contains(self, ability: AbilityIx) -> bool {
                ability.index() < Self::WIDTH && self & (1 << ability.index()) != 0
            }

            #[inline]
            fn with(self, ability: AbilityIx) -> Option<Self> {
                (ability.index() < Self::WIDTH).then(|| self | (1 << ability.index()))
            }

            #[inline]
            fn is_empty(self) -> bool {
                self == 0
            }

            #[inline]
            fn bits(self) -> u64 {
                u64::from(self)
            }

            #[inline]
            fn from_bits(bits: u64) -> Self {
                let low_bits = bits & u64::from(<$bits>::MAX);
                <$bits>::try_from(low_bits).expect("masked to the layout's ability width")
            }
        }
    )*};
}

ability_mask!(u16, u64);

/// An ability one card grants to other objects: the card's game index and the index into its
/// front face's [`beryl_card::Face::grants`] (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GrantRef<L: Layout = Small> {
    pub card: L::Card,
    pub grant: GrantIx,
}
