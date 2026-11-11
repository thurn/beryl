//! The records of the stack and of the triggers waiting to go on it (architecture §4.2):
//! plain data the rules build and the state stores.
//!
//! Transitional: by-w8gv.5.8 gives the stack and the trigger queues their mutators.

use beryl_card::{Castings, FaceAbility, FaceIx, Types};

use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Small};
use crate::object::ObjectRef;

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object or a player, and the instance of the word "target" it was
/// chosen for (an index into the ability's target specs). Four bytes, as an [`ObjectRef`]
/// with its padding used, where an enum of the two would take eight: a player has no slot
/// and the player in place of the incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target<L: Layout = Small> {
    pub spec: u8,
    slot: Option<L::Slot>,
    incarnation: u16,
}

const _: () = assert!(size_of::<Target<Small>>() == 4);

impl<L: Layout> Target<L> {
    pub fn object(spec: u8, object: ObjectRef<L>) -> Target<L> {
        Target {
            spec,
            slot: Some(object.slot),
            incarnation: object.incarnation,
        }
    }

    pub fn player(spec: u8, player: Seat) -> Target<L> {
        Target {
            spec,
            slot: None,
            incarnation: u16::from(player.get()),
        }
    }

    /// The targeted object, if it is one.
    pub fn as_object(self) -> Option<ObjectRef<L>> {
        self.slot.map(|slot| ObjectRef {
            slot,
            incarnation: self.incarnation,
        })
    }

    /// The targeted player, if it is one.
    pub fn as_player(self) -> Option<Seat> {
        match self.slot {
            None => Seat::try_new(self.incarnation as u8),
            Some(_) => None,
        }
    }
}

/// A spell or ability on the stack ([CR 405.1](https://yawgatog.com/resources/magic-rules/#R4051)):
/// "When a spell is cast, the physical card is put on the stack (see rule 601.2a). When an
/// ability is activated or triggers, it goes on top of the stack without any card associated
/// with it".
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackEntry<L: Layout = Small> {
    /// The spell's card, or the ability's source; an emblem's ability has none.
    pub object: Option<ObjectRef<L>>,
    pub controller: Seat,
    /// The ability of `face`, or `None` for a spell.
    pub ability: Option<FaceAbility>,
    /// The game index and face of the spell's card, or of the card that has the ability,
    /// which outlast the source ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)):
    /// "Once activated or triggered, an ability exists on the stack independently of its
    /// source."
    pub card: CardIx,
    pub face: FaceIx,
    /// For a triggered ability: what it triggered on.
    pub info: TriggerInfo<L>,
    pub targets: L::Queue<Target<L>, TARGETS>,
    /// The announced value of X ([CR 107.3a](https://yawgatog.com/resources/magic-rules/#R1073a)).
    pub x: u8,
    /// The face's casting options paid.
    pub costs: Castings,
    /// If the spell would be put into its owner's graveyard, it is exiled instead.
    pub exile: bool,
    /// Cast for its flashback cost, so it is exiled wherever else it would go as it leaves
    /// the stack ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "If the
    /// flashback cost was paid, exile this card instead of putting it anywhere else any time
    /// it would leave the stack."
    pub flashback: bool,
}

impl Copy for StackEntry<Small> {}

impl<L: Layout> StackEntry<L> {
    pub fn is_spell(&self) -> bool {
        self.ability.is_none()
    }

    /// The spell's card, if this is a spell.
    pub fn spell(&self) -> Option<ObjectRef<L>> {
        self.object.filter(|_| self.is_spell())
    }
}

/// Last-known information about a permanent that left the battlefield (architecture §5),
/// as it last existed there
/// ([CR 603.10](https://yawgatog.com/resources/magic-rules/#R60310)): the game "looks back in
/// time" for leaves-the-battlefield abilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lki {
    /// The game index of the card, the copiable values of a token copy.
    pub card: CardIx,
    pub power: i8,
    pub toughness: i8,
    pub types: Types,
    pub controller: Seat,
    pub token: bool,
}

/// What a triggered ability triggered on: the object and player the event involved, an
/// amount, and last-known information for look-back triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerInfo<L: Layout = Small> {
    pub object: Option<ObjectRef<L>>,
    pub player: Seat,
    pub amount: u8,
    pub lki: Lki,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger<L: Layout = Small> {
    /// The object with the ability; an emblem's trigger has none.
    pub source: Option<ObjectRef<L>>,
    pub card: CardIx,
    pub face: FaceIx,
    /// The index of the ability among the face's own.
    pub ability: u8,
    /// The player who controlled the source when it triggered
    /// ([CR 603.3a](https://yawgatog.com/resources/magic-rules/#R6033a)).
    pub controller: Seat,
    pub info: TriggerInfo<L>,
}
