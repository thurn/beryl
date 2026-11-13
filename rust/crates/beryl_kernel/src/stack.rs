//! The records of the stack and of the triggers waiting to go on it (architecture §4.2):
//! plain data the rules build and the state stores, with the mutators that file them (the
//! crate's contract, "Mutators").
//!
//! A spell is on the stack twice: its object is in the stack zone, which the zone move keeps
//! ([`State::move_object`]), and its [`StackEntry`] is in the stack's queue, which these
//! mutators keep. The entry goes on once the object is there and comes off before the object
//! leaves; the invariants hold a state to it ([`crate::invariants`]).

use beryl_card::{AbilityIx, Castings, FaceAbility, FaceIx, SpecIx, Types};

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::state::State;

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object or a player, and the instance of the word "target" it was
/// chosen for (an index into the ability's target specs).
///
/// Four bytes, where an enum of an [`ObjectRef`] and a [`Seat`] beside the spec would take
/// six: `slot` says which of the two it is, and `rest` is what else names that one. No field
/// is public, so [`Target::object`] and [`Target::player`] are the only way in and
/// [`Target::as_object`] and [`Target::as_player`] the only way out, and `rest` is never read
/// as the wrong one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target<L: Layout = Small> {
    spec: SpecIx,
    /// The targeted object's slot; none for a player.
    slot: Option<L::Slot>,
    /// The targeted object's incarnation, or the targeted player's seat.
    rest: u16,
}

const _: () = assert!(size_of::<Target<Small>>() == 4);

impl<L: Layout> Target<L> {
    #[inline]
    pub fn object(spec: SpecIx, object: ObjectRef<L>) -> Target<L> {
        Target {
            spec,
            slot: Some(object.slot),
            rest: object.incarnation,
        }
    }

    #[inline]
    pub fn player(spec: SpecIx, player: Seat) -> Target<L> {
        Target {
            spec,
            slot: None,
            rest: u16::from(player.get()),
        }
    }

    /// The instance of the word "target" it was chosen for.
    #[inline]
    pub fn spec(self) -> SpecIx {
        self.spec
    }

    /// The targeted object, if it is one.
    #[inline]
    pub fn as_object(self) -> Option<ObjectRef<L>> {
        self.slot.map(|slot| ObjectRef {
            slot,
            incarnation: self.rest,
        })
    }

    /// The targeted player, if it is one.
    #[inline]
    pub fn as_player(self) -> Option<Seat> {
        match self.slot {
            None => Seat::try_new(self.rest as u8),
            Some(_) => None,
        }
    }
}

/// The place of a chosen target in a stack entry's list of them
/// ([`StackEntry::targets`]), counting from 0 in the order they were chosen
/// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "The player announces
/// their choice of an appropriate object or player for each target the spell requires."
///
/// It is not the target's [`SpecIx`]: one instance of the word "target" may choose several
/// targets or none, so the two count differently, and neither is passed for the other.
///
/// ```compile_fail
/// use beryl_card::SpecIx;
/// use beryl_kernel::state::{ObjectRef, Target, TargetPlace};
///
/// // The first place is not the first instance of the word "target".
/// let target: Target = Target::object(TargetPlace::FIRST, ObjectRef::default());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TargetPlace(u8);

impl TargetPlace {
    /// The place of the first target chosen.
    pub const FIRST: TargetPlace = TargetPlace(0);

    /// The place `place`, counting from 0.
    #[inline]
    pub const fn new(place: u8) -> TargetPlace {
        assert!(
            (place as usize) < TARGETS,
            "a stack entry has TARGETS places"
        );
        TargetPlace(place)
    }

    /// The position in the list.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Each of `targets`, a stack entry's, with its place, in the order they were chosen
/// (CR 601.2c).
#[inline]
pub fn placed<L: Layout>(
    targets: &[Target<L>],
) -> impl Iterator<Item = (TargetPlace, Target<L>)> + '_ {
    debug_assert!(targets.len() <= TARGETS);
    (targets.iter().enumerate()).map(|(i, &target)| (TargetPlace(i as u8), target))
}

/// A set of the places of one stack entry's targets: a byte, a bit for each place. It has no
/// order of its own; [`placed`] gives the targets in the order they were chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TargetPlaces(u8);

const _: () = assert!(TARGETS <= 8, "a bit for each place");

impl TargetPlaces {
    pub const NONE: TargetPlaces = TargetPlaces(0);

    #[inline]
    pub fn insert(&mut self, place: TargetPlace) {
        self.0 |= 1 << place.0;
    }

    #[inline]
    pub fn contains(self, place: TargetPlace) -> bool {
        self.0 & (1 << place.0) != 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// An ability of a card, as it exists on the stack or waiting to go on it.
///
/// `A` says which abilities it can be: a [`FaceAbility`] for an activated ability, which may
/// be one another object grants, or an [`AbilityIx`] among the face's own for a triggered
/// ability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AbilityRef<L: Layout = Small, A = FaceAbility> {
    /// The object with the ability; an emblem's ability has none.
    pub source: Option<ObjectRef<L>>,
    /// The game index and face of the card that has the ability, which outlast the source
    /// ([CR 113.7a]): "Once activated or triggered, an ability exists on the stack
    /// independently of its source."
    ///
    /// [CR 113.7a]: https://yawgatog.com/resources/magic-rules/#R1137a
    pub card: L::Card,
    pub face: FaceIx,
    pub ability: A,
    /// The player who activated the ability, or who controlled its source when it triggered
    /// ([CR 603.3a]).
    ///
    /// [CR 603.3a]: https://yawgatog.com/resources/magic-rules/#R6033a
    pub controller: Seat,
}

/// What a [`StackEntry`] is: a spell, an activated ability or a triggered ability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StackItem<L: Layout = Small> {
    /// A spell: its card, in the stack zone, who controls it, the face cast and the face's
    /// casting options paid.
    Spell {
        object: ObjectRef<L>,
        controller: Seat,
        card: L::Card,
        face: FaceIx,
        costs: Castings,
    },
    /// An activated ability (CR 602.2a).
    Activated { of: AbilityRef<L> },
    /// A triggered ability, and what it triggered on (CR 603.3).
    Triggered {
        of: AbilityRef<L, AbilityIx>,
        info: TriggerInfo<L>,
    },
}

/// The value a fixed queue's unused places hold. No rule reads it.
impl<L: Layout> Default for StackItem<L> {
    fn default() -> Self {
        StackItem::Activated {
            of: AbilityRef {
                source: None,
                card: L::Card::default(),
                face: FaceIx::default(),
                ability: FaceAbility::own(0),
                controller: Seat::default(),
            },
        }
    }
}

/// A spell or ability on the stack ([CR 405.1]): "When a spell is cast, the physical card is
/// put on the stack (see rule 601.2a). When an ability is activated or triggers, it goes on
/// top of the stack without any card associated with it".
///
/// [CR 405.1]: https://yawgatog.com/resources/magic-rules/#R4051
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct StackEntry<L: Layout = Small> {
    pub item: StackItem<L>,
    pub targets: L::Queue<Target<L>, TARGETS>,
    /// The announced value of X ([CR 107.3a]), or for an enters ability the X of its spell
    /// ([CR 107.3m]). Spells and abilities both have one.
    ///
    /// [CR 107.3a]: https://yawgatog.com/resources/magic-rules/#R1073a
    /// [CR 107.3m]: https://yawgatog.com/resources/magic-rules/#R1073m
    pub x: u8,
}

impl Copy for StackEntry<Small> {}

impl<L: Layout> StackEntry<L> {
    /// An entry for `item`, with no targets chosen yet and X of 0.
    pub fn new(item: StackItem<L>) -> StackEntry<L> {
        StackEntry {
            item,
            targets: Default::default(),
            x: 0,
        }
    }

    /// The spell's card, if this is a spell.
    #[inline]
    pub fn spell(&self) -> Option<ObjectRef<L>> {
        match self.item {
            StackItem::Spell { object, .. } => Some(object),
            _ => None,
        }
    }

    /// The spell's card, or the ability's source; an emblem's ability has none.
    #[inline]
    pub fn object(&self) -> Option<ObjectRef<L>> {
        match self.item {
            StackItem::Spell { object, .. } => Some(object),
            StackItem::Activated { of } => of.source,
            StackItem::Triggered { of, .. } => of.source,
        }
    }

    /// The ability's source, if this is an ability that has one; never a spell's card.
    #[inline]
    pub fn source(&self) -> Option<ObjectRef<L>> {
        match self.item {
            StackItem::Spell { .. } => None,
            StackItem::Activated { of } => of.source,
            StackItem::Triggered { of, .. } => of.source,
        }
    }

    /// Who controls the spell or ability.
    #[inline]
    pub fn controller(&self) -> Seat {
        match self.item {
            StackItem::Spell { controller, .. } => controller,
            StackItem::Activated { of } => of.controller,
            StackItem::Triggered { of, .. } => of.controller,
        }
    }

    /// The game index of the spell's card, or of the card that has the ability.
    #[inline]
    pub fn card(&self) -> L::Card {
        match self.item {
            StackItem::Spell { card, .. } => card,
            StackItem::Activated { of } => of.card,
            StackItem::Triggered { of, .. } => of.card,
        }
    }

    /// The face of [`StackEntry::card`] the spell was cast as, or that has the ability.
    #[inline]
    pub fn face(&self) -> FaceIx {
        match self.item {
            StackItem::Spell { face, .. } => face,
            StackItem::Activated { of } => of.face,
            StackItem::Triggered { of, .. } => of.face,
        }
    }

    /// The ability of the card's face, or `None` for a spell.
    #[inline]
    pub fn ability(&self) -> Option<FaceAbility> {
        match self.item {
            StackItem::Spell { .. } => None,
            StackItem::Activated { of } => Some(of.ability),
            StackItem::Triggered { of, .. } => Some(FaceAbility::Own(of.ability)),
        }
    }

    /// What a triggered ability triggered on, if this is one.
    #[inline]
    pub fn info(&self) -> Option<&TriggerInfo<L>> {
        match &self.item {
            StackItem::Triggered { info, .. } => Some(info),
            _ => None,
        }
    }

    /// The casting options a spell's face was cast with; none for an ability.
    #[inline]
    pub fn costs(&self) -> Castings {
        match self.item {
            StackItem::Spell { costs, .. } => costs,
            _ => Castings::NONE,
        }
    }

    /// The slots it names ([`State::named`]): its spell's card or its ability's source,
    /// its targets, and the object a triggered ability triggered on.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        let objects = [self.object(), self.info().and_then(|info| info.object)];
        let targets = self.targets.iter().filter_map(|target| target.as_object());
        objects
            .into_iter()
            .flatten()
            .chain(targets)
            .map(|r| r.slot)
            .collect()
    }
}

/// Last-known information about a permanent that left the battlefield (architecture §5),
/// as it last existed there
/// ([CR 603.10](https://yawgatog.com/resources/magic-rules/#R60310)): the game "looks back in
/// time" for leaves-the-battlefield abilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lki<L: Layout = Small> {
    /// The game index of the card, the copiable values of a token copy.
    pub card: L::Card,
    pub power: i8,
    pub toughness: i8,
    pub types: PermanentTypes,
    pub controller: Seat,
    pub token: bool,
}

/// The card types of a permanent, in a byte: `beryl_card::Types` keeps every type a
/// permanent can have in its low byte
/// ([CR 110.4](https://yawgatog.com/resources/magic-rules/#R1104)): "Instant and sorcery
/// cards can't enter the battlefield and thus can't be permanents." So last-known
/// information, kept in every pending trigger and stack entry, stays as small as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PermanentTypes(u8);

impl PermanentTypes {
    /// A permanent's types: those of `types` a permanent can have.
    #[inline]
    pub fn of(types: Types) -> PermanentTypes {
        PermanentTypes(types.0.to_le_bytes()[0])
    }

    /// The card types.
    #[inline]
    pub fn types(self) -> Types {
        Types(u16::from(self.0))
    }
}

/// What a triggered ability triggered on: the object and player the event involved, an
/// amount, and last-known information for look-back triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerInfo<L: Layout = Small> {
    pub object: Option<ObjectRef<L>>,
    pub player: Seat,
    pub amount: u8,
    pub lki: Lki<L>,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger<L: Layout = Small> {
    /// The ability, among the face's own; its controller is the player who controlled the
    /// source when it triggered.
    pub of: AbilityRef<L, AbilityIx>,
    pub info: TriggerInfo<L>,
}

impl<L: Layout> PendingTrigger<L> {
    /// The slots it names ([`State::named`]): its source and the object it triggered on.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        let objects = [self.of.source, self.info.object].into_iter().flatten();
        objects.map(|r| r.slot).collect()
    }
}

impl<L: Layout> State<L> {
    /// The slots the stack's entries and the delayed triggered abilities name: a part of
    /// [`State::named`], which says what the rule is.
    pub(crate) fn named_by_stack(&self) -> Bitboard<L::Slot> {
        let mut named = Bitboard::EMPTY;
        for entry in self.stack.iter() {
            named |= entry.names();
        }
        for trigger in self.delayed.iter() {
            named |= trigger.names();
        }
        named
    }

    /// The slots the abilities that have triggered and wait to be put on the stack name:
    /// the other part.
    pub(crate) fn named_by_waiting_triggers(&self) -> Bitboard<L::Slot> {
        let mut named = Bitboard::EMPTY;
        for trigger in self.triggers.iter() {
            named |= trigger.names();
        }
        named
    }

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
    pub fn put_on_stack(&mut self, sink: &mut Sink<L>, entry: StackEntry<L>) -> Result<(), Full> {
        debug_assert!(
            entry
                .spell()
                .is_none_or(|spell| self.is_current(spell) && self.zones.stack.contains(spell.slot)),
            "a spell's entry without its card on the stack"
        );
        self.stack.push(entry)?;
        sink.arrived(|| {
            self.stack
                .as_slice()
                .last()
                .cloned()
                .expect("the entry just put")
        });
        Ok(())
    }

    /// Takes the top of the stack, to resolve it
    /// ([CR 608.1](https://yawgatog.com/resources/magic-rules/#R6081): "the spell or ability
    /// on top of the stack resolves") or because nothing legal can be chosen for a triggered
    /// ability going on it
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "the ability is
    /// simply removed from the stack"). A spell's card stays in the stack zone until the
    /// rules move it. The caller tells the sink which it was ([`Sink::enter_resolution`],
    /// [`Sink::departed`]).
    #[inline]
    pub fn take_top_of_stack(&mut self) -> Option<StackEntry<L>> {
        self.stack.pop()
    }

    /// Takes a spell's entry off the stack, wherever in it the spell is, for the rules to
    /// move its card elsewhere without its resolving: countered, returned to a hand, put
    /// into a library, exiled. Nothing if the spell is not on the stack. The entry says where
    /// such a spell goes, which is the rules' to read; its card leaves the stack zone with
    /// the move that must follow. The sink is told it left.
    #[inline]
    pub fn take_off_stack(
        &mut self,
        sink: &mut Sink<L>,
        spell: ObjectRef<L>,
    ) -> Option<StackEntry<L>> {
        let i = self.stack.iter().position(|e| e.spell() == Some(spell))?;
        sink.departed(|| i);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A target comes out as what it went in as, and never as the other.
    #[test]
    fn a_target_is_an_object_or_a_player() {
        let spec = SpecIx::new(1);
        let object = ObjectRef {
            slot: crate::ids::Slot::new(1),
            incarnation: 1,
        };
        let chosen: Target = Target::object(spec, object);
        assert_eq!((chosen.spec(), chosen.as_object()), (spec, Some(object)));
        assert_eq!(chosen.as_player(), None);
        let chosen: Target = Target::player(spec, Seat::P1);
        assert_eq!((chosen.spec(), chosen.as_player()), (spec, Some(Seat::P1)));
        assert_eq!(chosen.as_object(), None);
    }

    /// Targets are placed in the order they were chosen, and a set of places holds what was
    /// put in it.
    #[test]
    fn places_count_the_targets_in_the_order_chosen() {
        let targets: [Target; 3] = [
            Target::player(SpecIx::new(0), Seat::P0),
            Target::player(SpecIx::new(0), Seat::P1),
            Target::player(SpecIx::new(1), Seat::P0),
        ];
        let places: Vec<TargetPlace> = placed(&targets).map(|(place, _)| place).collect();
        assert_eq!(
            places,
            [TargetPlace::FIRST, TargetPlace::new(1), TargetPlace::new(2)]
        );
        let mut set = TargetPlaces::NONE;
        assert!(set.is_empty() && size_of::<TargetPlaces>() == 1);
        set.insert(places[2]);
        assert!(set.contains(places[2]) && !set.contains(places[0]) && !set.is_empty());
    }
}
