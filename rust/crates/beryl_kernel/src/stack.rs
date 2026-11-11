//! The records of the stack and of the triggers waiting to go on it (architecture §4.2):
//! plain data the rules build and the state stores, with the mutators that file them (the
//! crate's contract, "Mutators").
//!
//! A spell is on the stack twice: its object is in the stack zone, which the zone move keeps
//! ([`State::move_object`]), and its [`StackEntry`] is in the stack's queue, which these
//! mutators keep. The entry goes on once the object is there and comes off before the object
//! leaves; the invariants hold a state to it ([`crate::invariants`]).

use beryl_card::{AbilityIx, Castings, FaceAbility, FaceIx, SpecIx, Types};

use crate::fixed::Full;
use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::state::State;

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object or a player, and the instance of the word "target" it was
/// chosen for (an index into the ability's target specs). Four bytes, as an [`ObjectRef`]
/// with its padding used, where an enum of the two would take eight: a player has no slot
/// and the player in place of the incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target<L: Layout = Small> {
    pub spec: SpecIx,
    slot: Option<L::Slot>,
    incarnation: u16,
}

const _: () = assert!(size_of::<Target<Small>>() == 4);

impl<L: Layout> Target<L> {
    pub fn object(spec: SpecIx, object: ObjectRef<L>) -> Target<L> {
        Target {
            spec,
            slot: Some(object.slot),
            incarnation: object.incarnation,
        }
    }

    pub fn player(spec: SpecIx, player: Seat) -> Target<L> {
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
    /// The ability, among the face's own.
    pub ability: AbilityIx,
    /// The player who controlled the source when it triggered
    /// ([CR 603.3a](https://yawgatog.com/resources/magic-rules/#R6033a)).
    pub controller: Seat,
    pub info: TriggerInfo<L>,
}

impl<L: Layout> State<L> {
    /// The delayed triggered abilities waiting for their moment
    /// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
    #[inline]
    pub fn delayed(&self) -> &[PendingTrigger<L>] {
        self.delayed.as_slice()
    }

    /// Puts a spell or ability on top of the stack
    /// ([CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052)): "Each time an object
    /// is put on the stack, it's put on top of all objects already there." A spell's card is
    /// in the stack zone already ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a)).
    #[inline]
    pub fn put_on_stack(&mut self, entry: StackEntry<L>) -> Result<(), Full> {
        debug_assert!(
            entry
                .spell()
                .is_none_or(|spell| self.is_current(spell) && self.zones.stack.contains(spell.slot)),
            "a spell's entry without its card on the stack"
        );
        self.stack.push(entry)
    }

    /// Takes the top of the stack, to resolve it
    /// ([CR 608.1](https://yawgatog.com/resources/magic-rules/#R6081): "the spell or ability
    /// on top of the stack resolves") or because nothing legal can be chosen for a triggered
    /// ability going on it
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "the ability is
    /// simply removed from the stack"). A spell's card stays in the stack zone until the
    /// rules move it.
    #[inline]
    pub fn take_top_of_stack(&mut self) -> Option<StackEntry<L>> {
        self.stack.pop()
    }

    /// Takes a spell's entry off the stack, wherever in it the spell is, for the rules to
    /// move its card elsewhere without its resolving: countered, returned to a hand, put
    /// into a library, exiled. Nothing if the spell is not on the stack. The entry says where
    /// such a spell goes, which is the rules' to read; its card leaves the stack zone with
    /// the move that must follow.
    #[inline]
    pub fn take_off_stack(&mut self, spell: ObjectRef<L>) -> Option<StackEntry<L>> {
        let i = self.stack.iter().position(|e| e.spell() == Some(spell))?;
        Some(self.stack.remove(i))
    }

    /// Announces the value of X for the spell or ability being cast, the top of the stack
    /// (CR 601.2b).
    #[inline]
    pub fn announce_x(&mut self, x: u8) {
        self.casting_mut().x = x;
    }

    /// Announces a target of the spell or ability being cast, the top of the stack
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "The player
    /// announces their choice of an appropriate object or player for each target the spell
    /// requires."
    #[inline]
    pub fn announce_target(&mut self, target: Target<L>) -> Result<(), Full> {
        self.casting_mut().targets.push(target)
    }

    fn casting_mut(&mut self) -> &mut StackEntry<L> {
        self.stack
            .as_mut_slice()
            .last_mut()
            .expect("the spell or ability being cast")
    }

    /// An ability has triggered and waits to be put on the stack
    /// ([CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033)): "Once an ability has
    /// triggered, its controller puts it on the stack as an object that's not a card the
    /// next time a player would receive priority."
    #[inline]
    pub fn queue_trigger(&mut self, trigger: PendingTrigger<L>) -> Result<(), Full> {
        self.triggers.push(trigger)
    }

    /// Takes the waiting trigger at `index` among [`State::triggers`], for its controller to
    /// put it on the stack (CR 603.3b).
    #[inline]
    pub fn take_trigger(&mut self, index: usize) -> PendingTrigger<L> {
        self.triggers.remove(index)
    }

    /// Creates a delayed triggered ability
    /// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)): "An effect may create
    /// a delayed triggered ability that can do something at a later time."
    #[inline]
    pub fn delay_trigger(&mut self, trigger: PendingTrigger<L>) -> Result<(), Full> {
        self.delayed.push(trigger)
    }

    /// The delayed triggered abilities that `due` says trigger now do, each once, in the
    /// order they were created: they wait with the other triggers to be put on the stack
    /// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)).
    #[inline]
    pub fn fire_delayed(
        &mut self,
        mut due: impl FnMut(&PendingTrigger<L>) -> bool,
    ) -> Result<(), Full> {
        let mut i = 0;
        while i < self.delayed.len() {
            let d = self.delayed.as_slice()[i];
            if due(&d) {
                self.delayed.remove(i);
                self.triggers.push(d)?;
            } else {
                i += 1;
            }
        }
        Ok(())
    }
}
