//! The records of the stack and of the triggers waiting to go on it (architecture §4.2):
//! plain data the rules build and the state stores, with the mutators that file them (the
//! crate's contract, "Mutators").
//!
//! A spell is on the stack twice: its object is in the stack zone, which the zone move keeps
//! ([`State::move_object`]), and its [`StackEntry`] is in the stack's queue, which these
//! mutators keep. The entry goes on once the object is there and comes off before the object
//! leaves; the invariants hold a state to it ([`crate::invariants`]).

use beryl_card::{AbilityIx, Castings, FaceAbility, FaceIx, GrantIx, SpecIx};

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::state::State;

/// Targets per stack entry (architecture §4.2).
pub const TARGETS: usize = 8;

/// One chosen target: an object, a player or an ability on the stack, and the instance of the
/// word "target" it was chosen for (an index into the ability's target specs).
///
/// Four bytes, where an enum of an [`ObjectRef`], a [`Seat`] and a place beside the spec
/// would take six: `slot` says whether it is an object, and `rest` is what else names it. No
/// field is public, so [`Target::object`], [`Target::player`] and [`Target::ability`] are the
/// only way in and [`Target::as_object`], [`Target::as_player`] and [`Target::as_ability`]
/// the only way out, and `rest` is never read as the wrong one.
///
/// An ability is named by its place on the stack, counted from the bottom, since stack
/// entries have no identity of their own: a field in each entry would be copied in every
/// rollout. Only an entry above an ability can name it, so the names change only when an
/// entry below the top leaves, and [`State::take_off_stack`] repairs them then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Target<L: Layout = Small> {
    spec: SpecIx,
    /// The targeted object's slot; none for a player or an ability.
    slot: Option<L::Slot>,
    /// The targeted object's incarnation; the targeted player's seat; or, with
    /// [`Target::ABILITY`] set, the targeted ability's place on the stack, or
    /// [`Target::GONE`] once it has left.
    rest: u16,
}

const _: () = assert!(size_of::<Target<Small>>() == 4);

/// What an ability target names: its entry's place on the stack, counted from the bottom, or
/// nothing, once that entry has left the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityTarget {
    At(usize),
    /// The ability has left the stack, which makes it an illegal target
    /// ([CR 608.2b]): "A target that's no longer in the zone it was in when it was targeted
    /// is illegal."
    ///
    /// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
    Gone,
}

impl<L: Layout> Target<L> {
    /// The bit of `rest` that marks an ability.
    const ABILITY: u16 = 0x8000;
    /// `rest` for an ability that has left the stack.
    const GONE: u16 = u16::MAX;

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

    /// The activated or triggered ability at `place` on the stack, counted from the bottom.
    #[inline]
    pub fn ability(spec: SpecIx, place: usize) -> Target<L> {
        let place = u16::try_from(place).expect("a stack place fits in 15 bits");
        assert!(place < Self::GONE & !Self::ABILITY, "a stack place fits");
        Target {
            spec,
            slot: None,
            rest: Self::ABILITY | place,
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
            None if self.rest & Self::ABILITY == 0 => Seat::try_new(self.rest as u8),
            _ => None,
        }
    }

    /// The targeted ability, if it is one.
    #[inline]
    pub fn as_ability(self) -> Option<AbilityTarget> {
        match self.slot {
            None if self.rest == Self::GONE => Some(AbilityTarget::Gone),
            None if self.rest & Self::ABILITY != 0 => {
                Some(AbilityTarget::At(usize::from(self.rest & !Self::ABILITY)))
            }
            _ => None,
        }
    }

    /// The same target after the entry at `removed` left the stack from below the top: one
    /// that named it is gone, one that named a higher place names the place one lower, and
    /// any other is unchanged.
    #[inline]
    #[must_use]
    fn after_removal(self, removed: usize) -> Target<L> {
        match self.as_ability() {
            Some(AbilityTarget::At(place)) if place == removed => Target {
                rest: Self::GONE,
                ..self
            },
            Some(AbilityTarget::At(place)) if place > removed => {
                Target::ability(self.spec, place - 1)
            }
            _ => self,
        }
    }

    /// The same target in another layout: an ability's place and a player's seat are the
    /// same in every layout.
    #[inline]
    pub(crate) fn with_slot<M: Layout>(self, slot: Option<M::Slot>) -> Target<M> {
        Target {
            spec: self.spec,
            slot,
            rest: self.rest,
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
/// be one another object grants, or a [`TriggeredAbility`] for a triggered ability, one of the
/// face's own or one a copy exception gives ([`beryl_card::CopyChange::Has`]).
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

/// A triggered ability's [`FaceAbility`] in one byte, as the stack and the waiting triggers
/// keep it: an own ability's index, or 128 more than a grant's ([`FaceAbility::number`]). A
/// face has at most 64 abilities and a handful of grants. Two bytes in each of them cost a
/// [`Small`] state 312 bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggeredAbility(u8);

impl TriggeredAbility {
    /// `ability` in one byte.
    #[inline]
    pub const fn of(ability: FaceAbility) -> TriggeredAbility {
        TriggeredAbility(ability.number())
    }

    /// The face's own ability at `index`.
    #[inline]
    pub const fn own(index: AbilityIx) -> TriggeredAbility {
        TriggeredAbility::of(FaceAbility::Own(index))
    }

    /// The ability it names on `face`: an own one by its index, a grant out of line.
    #[inline]
    pub fn of_face(self, face: &'static beryl_card::Face) -> &'static beryl_card::Ability {
        if self.0 & 128 == 0 {
            &face.abilities[usize::from(self.0)]
        } else {
            face.ability(self.get())
        }
    }

    /// The ability it names.
    #[inline]
    pub const fn get(self) -> FaceAbility {
        if self.0 & 128 == 0 {
            FaceAbility::Own(AbilityIx::new(self.0))
        } else {
            FaceAbility::Grant(GrantIx::new(self.0 & 127))
        }
    }
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
        of: AbilityRef<L, TriggeredAbility>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StackEntry<L: Layout = Small> {
    pub item: StackItem<L>,
    pub targets: L::Queue<Target<L>, TARGETS>,
    /// The announced value of X ([CR 107.3a]), or for an enters ability the X of its spell
    /// ([CR 107.3m]). Spells and abilities both have one.
    ///
    /// [CR 107.3a]: https://yawgatog.com/resources/magic-rules/#R1073a
    /// [CR 107.3m]: https://yawgatog.com/resources/magic-rules/#R1073m
    pub x: u8,
    /// The text changes of an ability's source as the ability went on the stack, which its
    /// words read through from then on ([`State::entry_words`]): [`AS_PRINTED`], [`READ_NOW`],
    /// or the number of one of the state's captured sets, from 1. A spell reads its own words
    /// as they are now, whatever this holds.
    pub words: u8,
}

impl Copy for StackEntry<Small> {}

/// Its fields in order, and its words only when they are not [`AS_PRINTED`], as they always are
/// in a game without a text change: such a game hashes as it did before entries had words
/// (`beryl_bench` pins the hashes of its corpus).
impl<L: Layout> std::hash::Hash for StackEntry<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.item.hash(h);
        self.targets.hash(h);
        self.x.hash(h);
        if self.words != AS_PRINTED {
            self.words.hash(h);
        }
    }
}

/// An ability whose source's text was unchanged as it went on the stack, or whose source's
/// text no effect could change then: every entry's, in a game without a text change.
pub const AS_PRINTED: u8 = 0;

/// An ability that never went on the stack, as a mana ability resolves at once: its words are
/// its source's as they are now.
pub const READ_NOW: u8 = u8::MAX;

impl<L: Layout> StackEntry<L> {
    /// An entry for `item`, with no targets chosen yet and X of 0.
    pub fn new(item: StackItem<L>) -> StackEntry<L> {
        StackEntry {
            item,
            targets: Default::default(),
            x: 0,
            words: AS_PRINTED,
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
            StackItem::Triggered { of, .. } => Some(of.ability.get()),
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

/// What a triggered ability triggered on: the object and player the event involved, and an
/// amount. What an object that has left the battlefield was is read by the object
/// ([`State::last_known`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TriggerInfo<L: Layout = Small> {
    pub object: Option<ObjectRef<L>>,
    pub player: Seat,
    pub amount: u8,
}

/// An ability that has triggered and waits to be put on the stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PendingTrigger<L: Layout = Small> {
    /// The ability, among the face's own or, for one a copy exception gives, its grants. Its
    /// controller is the player who controlled the source when it triggered.
    pub of: AbilityRef<L, TriggeredAbility>,
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
    #[allow(clippy::inline_always)]
    #[inline(always)]
    pub fn put_on_stack(
        &mut self,
        sink: &mut Sink<L>,
        mut entry: StackEntry<L>,
    ) -> Result<(), Full> {
        debug_assert!(
            entry
                .spell()
                .is_none_or(|spell| self.is_current(spell) && self.zones.stack.contains(spell.slot)),
            "a spell's entry without its card on the stack"
        );
        if self.keeps_words() {
            self.words_on_stack(&mut entry)?;
        }
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
    /// into a library, exiled. Nothing if the spell is not on the stack. The ability targets
    /// of the entries above it are repaired, as [`Target`] says. The entry says where
    /// such a spell goes, which is the rules' to read; its card leaves the stack zone with
    /// the move that must follow. The sink is told it left.
    #[inline]
    pub fn take_off_stack(
        &mut self,
        sink: &mut Sink<L>,
        spell: ObjectRef<L>,
    ) -> Option<StackEntry<L>> {
        let i = self.stack.iter().position(|e| e.spell() == Some(spell))?;
        Some(self.remove_from_stack(sink, i))
    }

    /// Takes the activated or triggered ability at `place` off the stack, counted from the
    /// bottom, for an effect that counters it ([CR 701.6a]): "To counter a spell or ability
    /// means to cancel it, removing it from the stack." Nothing if no ability is there. The
    /// sink is told it left.
    ///
    /// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
    #[inline]
    pub fn take_ability_off_stack(
        &mut self,
        sink: &mut Sink<L>,
        place: usize,
    ) -> Option<StackEntry<L>> {
        let entry = self.stack.as_slice().get(place)?;
        entry
            .spell()
            .is_none()
            .then(|| self.remove_from_stack(sink, place))
    }

    /// Removes the entry at `i`, wherever in the stack it is, and repairs the ability targets
    /// of the entries above it, the only ones that can name it or a place above it
    /// ([`Target::after_removal`]). A target that named the removed entry is gone, which the
    /// rules read as illegal ([CR 608.2b]): "A target that's no longer in the zone it was in
    /// when it was targeted is illegal." Removing the top entry repairs nothing.
    ///
    /// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
    fn remove_from_stack(&mut self, sink: &mut Sink<L>, i: usize) -> StackEntry<L> {
        sink.departed(|| i);
        let removed = self.stack.remove(i);
        for entry in &mut self.stack.as_mut_slice()[i..] {
            for target in entry.targets.as_mut_slice() {
                *target = target.after_removal(i);
            }
        }
        self.settle_captured_words();
        removed
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
        assert_eq!((chosen.as_object(), chosen.as_ability()), (None, None));
        let chosen: Target = Target::ability(spec, 1);
        assert_eq!(
            (chosen.spec(), chosen.as_ability()),
            (spec, Some(AbilityTarget::At(1)))
        );
        assert_eq!((chosen.as_object(), chosen.as_player()), (None, None));
    }

    /// When an entry leaves from below the top, a target that named it is gone, one that
    /// named a place above it moves down one, and one below it or not an ability stays.
    #[test]
    fn a_removal_below_an_ability_target_repairs_it() {
        let spec = SpecIx::new(0);
        let at = |place| Target::<Small>::ability(spec, place);
        assert_eq!(
            at(2).after_removal(2).as_ability(),
            Some(AbilityTarget::Gone)
        );
        assert_eq!(at(3).after_removal(1), at(2));
        assert_eq!(at(1).after_removal(2), at(1));
        let gone = at(2).after_removal(2);
        assert_eq!(gone.after_removal(0), gone);
        let player = Target::<Small>::player(spec, Seat::P0);
        assert_eq!(player.after_removal(0), player);
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
