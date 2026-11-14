//! Objects: the base data of each slot, a reference to one incarnation of it, and the layer
//! results of a permanent (architecture §4.2, §5, §9).
//!
//! Only the kernel writes an object: through [`State`](crate::State)'s mutators, the zone and
//! chance chokepoints and the board door. No `&mut Object` leaves the
//! crate. Other crates read an object through the getters below.

use std::fmt::Debug;
use std::hash::Hash;

use beryl_card::{AbilityIx, Colors, FaceIx, GrantIx, Keywords, Supertypes, Types, Zone};

use crate::counters::Counters;
use crate::fixed::Full;
use crate::ids::{CardId, CardIx, Seat, Slot};
use crate::layout::{Layout, Small};

/// The [`Object::battlefield_index`] of a slot not on the battlefield.
pub(crate) const OFF_BATTLEFIELD: u16 = u16::MAX;

/// A status an object keeps in its [`ObjectFlags`]. Each variant's value is its bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Flag {
    /// [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105): "A permanent's status is its
    /// physical state. There are four status categories, each of which has two possible values:
    /// tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased out."
    Tapped = 1 << 0,
    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    SummoningSick = 1 << 1,
    /// A token ([CR 111.1]): "a marker used to represent any permanent that isn't
    /// represented by a card."
    ///
    /// [CR 111.1]: https://yawgatog.com/resources/magic-rules/#R1111
    Token = 1 << 3,
    /// A copy whose copiable values are 1/1 by exception, such as an offspring token
    /// ([CR 707.9b]): "Some copy effects modify a characteristic as part of the copying
    /// process. The final set of values for that characteristic becomes part of the copiable
    /// values of the copy."
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    OneOne = 1 << 4,
}

/// An object's statuses, one bit per [`Flag`]. Combat is no flag: it is the object's
/// [`CombatRole`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectFlags(u8);

impl ObjectFlags {
    pub(crate) const NONE: ObjectFlags = ObjectFlags(0);

    #[inline]
    pub fn has(self, flag: Flag) -> bool {
        self.0 & flag as u8 != 0
    }

    #[inline]
    pub(crate) fn set(&mut self, flag: Flag, on: bool) {
        if on {
            self.0 |= flag as u8;
        } else {
            self.0 &= !(flag as u8);
        }
    }

    /// The flags an object keeps as it changes zones: being a token, and a copy's 1/1
    /// exception. A token stays a token wherever it goes, until it ceases to exist
    /// ([CR 111.7]).
    ///
    /// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
    #[inline]
    pub(crate) fn kept_across_zones(self) -> ObjectFlags {
        ObjectFlags(self.0 & (Flag::Token as u8 | Flag::OneOne as u8))
    }
}

/// What an attacking creature attacks ([CR 506.2]): "that player, planeswalkers they
/// control, and battles they protect may be attacked."
///
/// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttackTarget<L: Layout = Small> {
    Player(Seat),
    Planeswalker(L::Slot),
    /// The planeswalker it attacked was removed from combat ([CR 506.4c]): "It continues
    /// to be an attacking creature, although it is not attacking any player, planeswalker,
    /// or battle."
    ///
    /// [CR 506.4c]: https://yawgatog.com/resources/magic-rules/#R5064c
    Removed,
}

/// The attackers a blocking creature blocks, in the order it was declared to block them.
/// A creature blocks one attacker unless an effect lets it block more ([CR 509.1a]), and
/// this holds up to [`SlotSet::CAPACITY`].
///
/// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotSet<L: Layout = Small>([Option<L::Slot>; SlotSet::<Small>::CAPACITY]);

impl<L: Layout> SlotSet<L> {
    pub const CAPACITY: usize = 3;
    pub const EMPTY: SlotSet<L> = SlotSet([None; SlotSet::<Small>::CAPACITY]);

    #[inline]
    pub fn contains(&self, slot: L::Slot) -> bool {
        self.0.contains(&Some(slot))
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0[0].is_none()
    }

    /// The first slot added that is still in the set.
    #[inline]
    pub fn first(&self) -> Option<L::Slot> {
        self.0[0]
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = L::Slot> + '_ {
        self.0.iter().map_while(|&slot| slot)
    }

    /// Adds `slot` after the others, unless it is in the set already. [`Full`] if the set
    /// already holds [`SlotSet::CAPACITY`] slots.
    #[inline]
    pub(crate) fn insert(&mut self, slot: L::Slot) -> Result<(), Full> {
        if self.contains(slot) {
            return Ok(());
        }
        let free = self.0.iter_mut().find(|s| s.is_none()).ok_or(Full)?;
        *free = Some(slot);
        Ok(())
    }

    /// Removes `slot`, keeping the others in order.
    #[inline]
    pub(crate) fn remove(&mut self, slot: L::Slot) {
        if let Some(i) = self.0.iter().position(|&s| s == Some(slot)) {
            self.0.copy_within(i + 1.., i);
            self.0[Self::CAPACITY - 1] = None;
        }
    }

    #[inline]
    fn map<M: Layout>(&self, f: impl Fn(L::Slot) -> M::Slot) -> SlotSet<M> {
        SlotSet(self.0.map(|s| s.map(&f)))
    }
}

/// What a permanent is doing in combat ([CR 506.3]): "Only a creature can attack or
/// block." A creature is attacking, blocking or neither, never both.
///
/// [CR 506.3]: https://yawgatog.com/resources/magic-rules/#R5063
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CombatRole<L: Layout = Small> {
    #[default]
    None,
    /// An attacking creature ([CR 508.1k]): "Each chosen creature still controlled by the
    /// active player becomes an attacking creature."
    ///
    /// [CR 508.1k]: https://yawgatog.com/resources/magic-rules/#R5081k
    Attacking {
        target: AttackTarget<L>,
        /// Blocked ([CR 509.1h]): "An attacking creature with one or more creatures
        /// declared as blockers for it becomes a blocked creature ... A creature remains
        /// blocked even if all the creatures blocking it are removed from combat."
        ///
        /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
        blocked: bool,
        /// Assigned its combat damage in the first-strike step ([CR 510.4]).
        ///
        /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
        struck_first: bool,
    },
    /// A blocking creature ([CR 509.1g]): "Each chosen creature still controlled by the
    /// defending player becomes a blocking creature." It blocks no attacker between its
    /// declaration and the choice of what it blocks, and none once those attackers have
    /// left combat ([CR 506.4]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    /// [CR 506.4]: https://yawgatog.com/resources/magic-rules/#R5064
    Blocking {
        attackers: SlotSet<L>,
        /// Assigned its combat damage in the first-strike step ([CR 510.4]).
        ///
        /// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
        struck_first: bool,
    },
}

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
            _ => (None, None),
        };
        walker.into_iter().chain(attackers.into_iter().flatten())
    }

    /// Stops naming `slot`, which has left combat ([CR 506.4]): "A planeswalker or battle
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
            _ => {}
        }
    }

    /// The same role with its slots renumbered by `f`.
    #[inline]
    pub(crate) fn map<M: Layout>(&self, f: impl Fn(L::Slot) -> M::Slot) -> CombatRole<M> {
        match *self {
            CombatRole::None => CombatRole::None,
            CombatRole::Attacking {
                target,
                blocked,
                struck_first,
            } => CombatRole::Attacking {
                target: match target {
                    AttackTarget::Player(p) => AttackTarget::Player(p),
                    AttackTarget::Planeswalker(w) => AttackTarget::Planeswalker(f(w)),
                    AttackTarget::Removed => AttackTarget::Removed,
                },
                blocked,
                struck_first,
            },
            CombatRole::Blocking {
                attackers,
                struck_first,
            } => CombatRole::Blocking {
                attackers: attackers.map::<M>(f),
                struck_first,
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
        match *self {
            CombatRole::None => (0, None),
            CombatRole::Attacking {
                target,
                blocked,
                struck_first,
            } => {
                let blocked = if blocked { BLOCKED } else { 0 };
                let (walker, slot) = match target {
                    AttackTarget::Player(_) => (0, None),
                    AttackTarget::Planeswalker(w) => (ATTACKS_PLANESWALKER, Some(w)),
                    AttackTarget::Removed => (ATTACKS_PLANESWALKER, None),
                };
                (ATTACKING | blocked | walker | first(struck_first), slot)
            }
            CombatRole::Blocking {
                attackers,
                struck_first,
            } => (BLOCKING | first(struck_first), attackers.first()),
        }
    }
}

/// The base data of the object in one slot (architecture §4.2).
///
/// The fields are `pub(crate)`: other crates read an object through its getters, and only
/// the kernel can write one, since no `&mut Object` leaves the crate.
///
/// Aligned to 4 bytes, so a `Small` object is 24 bytes rather than 22: the rollout
/// benchmark counts about 118,000 fewer instructions indexing a table of 24-byte objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(align(4))]
pub struct Object<L: Layout = Small> {
    /// Index into the game table: the definition its values are read from. The kernel's
    /// own: others ask a query ([`crate::query`]) or [`crate::Observation::card`].
    pub(crate) card: L::Card,
    /// Bumped on every zone change (CR 400.7), so stale references are detectable.
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
    /// Bumped on every zone change ([CR 400.7]): "An object that moves from one zone to another
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

    /// [CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083): "The owner of a card in the
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

    /// [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105): "A permanent's status is its
    /// physical state. There are four status categories, each of which has two possible values:
    /// tapped/untapped, flipped/unflipped, face up/face down, and phased in/phased out."
    #[inline]
    pub fn is_tapped(&self) -> bool {
        self.flags.has(Flag::Tapped)
    }

    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6]).
    ///
    /// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
    #[inline]
    pub fn is_summoning_sick(&self) -> bool {
        self.flags.has(Flag::SummoningSick)
    }

    /// [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "a marker used to represent
    /// any permanent that isn't represented by a card."
    #[inline]
    pub fn is_token(&self) -> bool {
        self.flags.has(Flag::Token)
    }

    /// A copy whose copiable values are 1/1 by exception ([CR 707.9b]).
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[inline]
    pub fn is_one_one(&self) -> bool {
        self.flags.has(Flag::OneOne)
    }

    /// What it is doing in combat.
    #[inline]
    pub fn combat(&self) -> &CombatRole<L> {
        &self.combat
    }

    /// An attacking creature ([CR 506.3]).
    ///
    /// [CR 506.3]: https://yawgatog.com/resources/magic-rules/#R5063
    #[inline]
    pub fn is_attacking(&self) -> bool {
        matches!(self.combat, CombatRole::Attacking { .. })
    }

    /// A blocking creature ([CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn is_blocking(&self) -> bool {
        matches!(self.combat, CombatRole::Blocking { .. })
    }

    /// An attacking creature that has been blocked ([CR 509.1h]).
    ///
    /// [CR 509.1h]: https://yawgatog.com/resources/magic-rules/#R5091h
    #[inline]
    pub fn is_blocked(&self) -> bool {
        matches!(self.combat, CombatRole::Attacking { blocked: true, .. })
    }

    /// What an attacking creature attacks ([CR 506.2]); `None` if it is not attacking.
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    #[inline]
    pub fn attack_target(&self) -> Option<AttackTarget<L>> {
        match self.combat {
            CombatRole::Attacking { target, .. } => Some(target),
            _ => None,
        }
    }

    /// The planeswalker an attacking creature attacks ([CR 506.2]).
    ///
    /// [CR 506.2]: https://yawgatog.com/resources/magic-rules/#R5062
    #[inline]
    pub fn attacked_planeswalker(&self) -> Option<L::Slot> {
        match self.combat {
            CombatRole::Attacking {
                target: AttackTarget::Planeswalker(walker),
                ..
            } => Some(walker),
            _ => None,
        }
    }

    /// Whether it is a blocking creature that blocks `attacker` ([CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn blocks(&self, attacker: L::Slot) -> bool {
        matches!(self.combat, CombatRole::Blocking { attackers, .. } if attackers.contains(attacker))
    }

    /// The attackers a blocking creature blocks, none for any other object ([CR 509.1g]).
    ///
    /// [CR 509.1g]: https://yawgatog.com/resources/magic-rules/#R5091g
    #[inline]
    pub fn blocked_attackers(&self) -> SlotSet<L> {
        match self.combat {
            CombatRole::Blocking { attackers, .. } => attackers,
            _ => SlotSet::EMPTY,
        }
    }

    /// Dealt its combat damage in the first-strike step ([CR 510.4]).
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

    /// Damage marked this turn ([CR 120.6]): "Damage marked on a creature remains until the cleanup
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

    /// The object an Aura or Equipment is attached to ([CR 701.3]).
    ///
    /// [CR 701.3]: https://yawgatog.com/resources/magic-rules/#R7013
    #[inline]
    pub fn attached_to(&self) -> Option<L::Slot> {
        self.attached_to
    }

    /// When it entered the battlefield or last turned face up, among the layers' timestamps
    /// ([CR 613.7]).
    ///
    /// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
    #[inline]
    pub fn timestamp(&self) -> u16 {
        self.timestamp
    }

    /// Which face is up ([CR 712.6]), or [`FaceIx::DOWN`] ([CR 708.2]).
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
const _: () = assert!(size_of::<Object<Small>>() == 24);

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
        incarnation.hash(h);
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
        timestamp.hash(h);
        face.hash(h);
        combat_slot.hash(h);
    }
}

/// A reference to one incarnation of the object in a slot (architecture §5). A reference
/// whose incarnation no longer matches names an object that has left its zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectRef<L: Layout = Small> {
    pub slot: L::Slot,
    pub incarnation: u16,
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
    /// ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing
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
            #[allow(clippy::cast_possible_truncation)]
            fn from_bits(bits: u64) -> Self {
                bits as $bits
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
