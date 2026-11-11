//! Objects: the base data of each slot, a reference to one incarnation of it, and the layer
//! results of a permanent (architecture §4.2, §5, §9).
//!
//! Only the kernel writes an object: through [`State`](crate::State)'s mutators, the zone and
//! chance chokepoints, the board door and the legacy accessor. No `&mut Object` leaves the
//! crate. Other crates read an object through the getters below.

use beryl_card::{
    Colors, CounterKind, FaceIx, GrantIx, Keywords, Subtypes, Supertypes, Types, Zone,
};

use crate::ids::{CardId, CardIx, Seat, Slot};
use crate::layout::{Layout, Small};

/// The [`Object::battlefield_index`] of a slot not on the battlefield.
pub(crate) const OFF_BATTLEFIELD: u16 = u16::MAX;

/// An object's status and combat flags, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectFlags(pub u32);

impl ObjectFlags {
    pub const TAPPED: u32 = 1 << 0;
    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub const SUMMONING_SICK: u32 = 1 << 1;
    /// An attacking creature
    /// ([CR 506.3](https://yawgatog.com/resources/magic-rules/#R5063)): "Only a creature can
    /// attack or block." What it attacks is in `combat`.
    pub const ATTACKING: u32 = 1 << 2;
    /// A token ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)): "a marker
    /// used to represent any permanent that isn't represented by a card."
    pub const TOKEN: u32 = 1 << 3;
    /// A copy whose copiable values are 1/1 by exception, such as an offspring token
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "Some copy effects
    /// modify a characteristic as part of the copying process. The final set of values for
    /// that characteristic becomes part of the copiable values of the copy."
    pub const ONE_ONE: u32 = 1 << 4;
    /// Exiled until the permanent in `linked` leaves the battlefield (CR 610.3).
    pub const EXILED_UNTIL: u32 = 1 << 5;
    /// Exiled by an ability of the object in `linked`, whose linked ability refers to the
    /// cards "exiled with" it ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)).
    pub const EXILED_WITH: u32 = 1 << 6;
    /// A blocking creature ([CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g)).
    /// The attacker it blocks is in `combat`.
    pub const BLOCKING: u32 = 1 << 8;
    /// An attacking creature that has been blocked
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)): "An attacking
    /// creature with one or more creatures declared as blockers for it becomes a blocked
    /// creature ... A creature remains blocked even if all the creatures blocking it are
    /// removed from combat."
    pub const BLOCKED: u32 = 1 << 9;
    /// An attacker attacking a planeswalker rather than a player. If that planeswalker has
    /// left the battlefield, `combat` is `None` and the creature attacks nothing.
    pub const ATTACKS_PLANESWALKER: u32 = 1 << 10;
    /// Had first strike as the first-strike combat damage step began, so it deals no damage
    /// in the second ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)).
    pub const STRUCK_FIRST: u32 = 1 << 11;
    /// Every flag that combat sets, cleared as combat ends.
    pub const COMBAT: u32 = Self::ATTACKING
        | Self::BLOCKING
        | Self::BLOCKED
        | Self::ATTACKS_PLANESWALKER
        | Self::STRUCK_FIRST;
    /// Entered the battlefield this turn.
    pub const ENTERED_THIS_TURN: u32 = 1 << 16;
    /// A loyalty ability of this permanent was activated this turn
    /// ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)).
    pub const LOYALTY_ACTIVATED: u32 = 1 << 17;
    /// The flags milestone 4 (by-w8gv.7.1) moves to memos and trackers. No mutator sets
    /// them: they are written through [`State::legacy_mut`](crate::State::legacy_mut)
    /// alone.
    pub const LEGACY: u32 = Self::EXILED_UNTIL | Self::EXILED_WITH | Self::LOYALTY_ACTIVATED;

    #[inline]
    pub fn has(self, flag: u32) -> bool {
        self.0 & flag != 0
    }

    #[inline]
    pub fn set(&mut self, flag: u32, on: bool) {
        if on {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

/// Counters on an object: the common kinds, and one (kind, count) pair for the long tail,
/// which milestone 4 (by-w8gv.7.1) moves to a store and only
/// [`State::legacy_mut`](crate::State::legacy_mut) writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Counters {
    pub plus_one: u8,
    pub minus_one: u8,
    pub loyalty: u8,
    pub(crate) other_kind: CounterKind,
    pub(crate) other: u8,
}

impl Counters {
    /// Whether a kind is one of the long tail, kept in the one (kind, count) pair.
    #[inline]
    pub const fn is_long_tail(kind: CounterKind) -> bool {
        !matches!(
            kind,
            CounterKind::PlusOne | CounterKind::MinusOne | CounterKind::Loyalty
        )
    }

    /// The long-tail kind this object has counters of, and how many.
    #[inline]
    pub fn long_tail(self) -> Option<(CounterKind, u8)> {
        (self.other > 0).then_some((self.other_kind, self.other))
    }

    /// How many counters of a kind there are.
    #[inline]
    pub fn get(self, kind: CounterKind) -> u8 {
        match kind {
            CounterKind::PlusOne => self.plus_one,
            CounterKind::MinusOne => self.minus_one,
            CounterKind::Loyalty => self.loyalty,
            _ if self.other_kind == kind => self.other,
            _ => 0,
        }
    }

    /// The count of a kind, to change. One object carries one long-tail kind at a time,
    /// which the cube's cards never exceed; adding a second kind replaces the first.
    #[inline]
    pub(crate) fn get_mut(&mut self, kind: CounterKind) -> &mut u8 {
        match kind {
            CounterKind::PlusOne => &mut self.plus_one,
            CounterKind::MinusOne => &mut self.minus_one,
            CounterKind::Loyalty => &mut self.loyalty,
            _ => {
                if self.other_kind != kind {
                    debug_assert_eq!(self.other, 0, "a second long-tail counter kind");
                    self.other_kind = kind;
                    self.other = 0;
                }
                &mut self.other
            }
        }
    }
}

/// The base data of the object in one slot (architecture §4.2).
///
/// The fields are `pub(crate)`: other crates read an object through its getters, and only
/// the kernel can write one, since no `&mut Object` leaves the crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// The object that exiled, imprinted or hid this one.
    pub(crate) linked: Option<L::Slot>,
    /// Battlefield entry order, for layers.
    pub(crate) timestamp: u16,
    /// Which face is up.
    pub(crate) face: FaceIx,
    /// In combat: the planeswalker an attacker attacks, or the attacker a blocker blocks;
    /// `None` for an attacker attacking a player, or once that object has left the
    /// battlefield.
    pub(crate) combat: Option<L::Slot>,
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
        flags: ObjectFlags(0),
        damage: 0,
        counters: Counters {
            plus_one: 0,
            minus_one: 0,
            loyalty: 0,
            other_kind: CounterKind::PlusOne,
            other: 0,
        },
        attached_to: None,
        linked: None,
        timestamp: 0,
        face: FaceIx::FRONT,
        combat: None,
        battlefield_index: OFF_BATTLEFIELD,
    };
}

/// What the rules give an object, to read. A field has one getter, and a status flag one
/// each, but its definition's index and its base controller: those are answered by
/// [`State`](crate::State)'s queries ([`crate::query`]).
impl<L: Layout> Object<L> {
    /// Bumped on every zone change
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that moves
    /// from one zone to another becomes a new object".
    #[inline]
    pub fn incarnation(&self) -> u16 {
        self.incarnation
    }

    #[inline]
    pub fn zone(&self) -> Zone {
        self.zone
    }

    /// [CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083): "The owner of a card in
    /// the game is the player who started the game with it in their deck."
    #[inline]
    pub fn owner(&self) -> Seat {
        self.owner
    }

    /// Every status and combat flag at once.
    #[inline]
    pub fn flags(&self) -> ObjectFlags {
        self.flags
    }

    /// [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105): "A permanent's status
    /// is its physical state. There are four status categories, each of which has two
    /// possible values: tapped/untapped, flipped/unflipped, face up/face down, and phased
    /// in/phased out."
    #[inline]
    pub fn is_tapped(&self) -> bool {
        self.flags.has(ObjectFlags::TAPPED)
    }

    /// Not under its controller's control continuously since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    #[inline]
    pub fn is_summoning_sick(&self) -> bool {
        self.flags.has(ObjectFlags::SUMMONING_SICK)
    }

    /// [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "a marker used to
    /// represent any permanent that isn't represented by a card."
    #[inline]
    pub fn is_token(&self) -> bool {
        self.flags.has(ObjectFlags::TOKEN)
    }

    /// A copy whose copiable values are 1/1 by exception
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)).
    #[inline]
    pub fn is_one_one(&self) -> bool {
        self.flags.has(ObjectFlags::ONE_ONE)
    }

    #[inline]
    pub fn entered_this_turn(&self) -> bool {
        self.flags.has(ObjectFlags::ENTERED_THIS_TURN)
    }

    /// An attacking creature ([CR 506.3](https://yawgatog.com/resources/magic-rules/#R5063)).
    #[inline]
    pub fn is_attacking(&self) -> bool {
        self.flags.has(ObjectFlags::ATTACKING)
    }

    /// A blocking creature
    /// ([CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g)).
    #[inline]
    pub fn is_blocking(&self) -> bool {
        self.flags.has(ObjectFlags::BLOCKING)
    }

    /// An attacking creature that has been blocked
    /// ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)).
    #[inline]
    pub fn is_blocked(&self) -> bool {
        self.flags.has(ObjectFlags::BLOCKED)
    }

    /// An attacker attacking a planeswalker rather than a player
    /// ([CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062)).
    #[inline]
    pub fn attacks_planeswalker(&self) -> bool {
        self.flags.has(ObjectFlags::ATTACKS_PLANESWALKER)
    }

    /// Dealt its combat damage in the first-strike step
    /// ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)).
    #[inline]
    pub fn struck_first(&self) -> bool {
        self.flags.has(ObjectFlags::STRUCK_FIRST)
    }

    /// Damage marked this turn
    /// ([CR 120.6](https://yawgatog.com/resources/magic-rules/#R1206)): "Damage marked on a
    /// creature remains until the cleanup step".
    #[inline]
    pub fn damage(&self) -> u8 {
        self.damage
    }

    #[inline]
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// How many counters of a kind are on it
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)).
    #[inline]
    pub fn counters_of(&self, kind: CounterKind) -> u8 {
        self.counters.get(kind)
    }

    /// The object an Aura or Equipment is attached to
    /// ([CR 701.3](https://yawgatog.com/resources/magic-rules/#R7013)).
    #[inline]
    pub fn attached_to(&self) -> Option<L::Slot> {
        self.attached_to
    }

    /// The object that exiled this one
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a),
    /// [CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)).
    #[inline]
    pub fn linked(&self) -> Option<L::Slot> {
        self.linked
    }

    /// When it entered the battlefield or last turned face up, among the layers' timestamps
    /// ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)).
    #[inline]
    pub fn timestamp(&self) -> u16 {
        self.timestamp
    }

    /// Which face is up ([CR 712.6](https://yawgatog.com/resources/magic-rules/#R7126)), or
    /// [`FaceIx::DOWN`] ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)).
    #[inline]
    pub fn face(&self) -> FaceIx {
        self.face
    }

    /// In combat: the planeswalker an attacker attacks, or the attacker a blocker blocks
    /// ([CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062),
    /// [CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g)).
    #[inline]
    pub fn combat(&self) -> Option<L::Slot> {
        self.combat
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
            linked,
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
        flags.hash(h);
        damage.hash(h);
        counters.hash(h);
        attached_to.hash(h);
        linked.hash(h);
        timestamp.hash(h);
        face.hash(h);
        combat.hash(h);
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
    pub subtypes: Subtypes,
    /// The abilities other objects grant this permanent (§9), in the order granted.
    pub granted: [Option<GrantRef<L>>; 4],
}

/// An ability one card grants to other objects: the card's game index and the index into its
/// front face's [`beryl_card::Face::grants`] (§9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GrantRef<L: Layout = Small> {
    pub card: L::Card,
    pub grant: GrantIx,
}
