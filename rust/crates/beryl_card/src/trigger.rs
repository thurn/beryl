//! Triggers: what a triggered ability triggers on, said as an event, the object it is about
//! and the player it happens to or who does it.

use crate::effect::{Condition, Players};
use crate::filter::Filter;
use crate::ids::{TurnStep, Zone};

/// What a triggered ability triggers on
/// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)): "Whenever a game event
/// or game state matches a triggered ability's trigger event, that ability automatically
/// triggers."
///
/// A product, so that a card says a new trigger with entries that exist: "whenever an
/// opponent casts a noncreature spell" is [`TriggerEvent::Casts`], a filter and
/// [`Players::EachOpponent`], not an entry of its own. The engine matches a trigger by its
/// event's kind, and a game's card table refuses a product no code of the engine matches
/// (`beryl_kernel`'s `TableError::Trigger`), so a definition that compiles and builds a
/// table triggers as it reads.
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    /// What happens.
    pub event: TriggerEvent,
    /// Which object it happens to or which does it, for an event that has one.
    pub subject: Subject,
    /// Which player it happens to or who does it, for an event that has one: any of these
    /// players, read for the ability's controller. [`Players::EachPlayer`] is "a player",
    /// whoever it is, and what a trigger says of an event that has no player.
    pub whose: Players,
    /// Whether this is a delayed triggered ability, which an effect makes and no object has
    /// printed ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)): "An effect
    /// may create a delayed triggered ability that can do something at a later time." It is
    /// said like any other, with the word "next", and triggers once
    /// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A delayed
    /// triggered ability will trigger only once--the next time its trigger event
    /// occurs--unless it has a stated duration".
    pub delayed: bool,
    /// Whether this delayed ability can trigger only in the extra turn an effect schedules.
    pub exact_turn: bool,
}

/// The event of a trigger: one of the kinds of event the engine raises. An entry names what
/// happens and never which object or player it happens to ([`Subject`], [`Players`]).
///
/// Two events are the same when they are the same entry, and for [`TriggerEvent::State`] the
/// same condition: the same static, compared by address, since a [`Condition`] has no
/// equality of its own.
#[derive(Clone, Copy, Debug)]
pub enum TriggerEvent {
    /// "When [subject] enters"
    /// ([CR 603.6a](https://yawgatog.com/resources/magic-rules/#R6036a)).
    Enters,
    /// "When [subject] dies": a leaves-the-battlefield ability, which looks back in time
    /// ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)).
    Dies,
    /// "When [subject] leaves the battlefield", for any zone: a leaves-the-battlefield
    /// ability, which looks back in time (CR 603.10a).
    Leaves,
    /// "Whenever [subject] attacks"
    /// ([CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a)): it "triggers if
    /// that creature is declared as an attacker." Once for each such creature.
    Attacks,
    /// "Whenever [whose] attack": once for a declaration of attackers, however many
    /// ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)): it "triggers if
    /// one or more creatures that player controls are declared as attackers." The subject is
    /// the attackers it counts: "whenever you attack with one or more [subject]".
    PlayerAttacks,
    /// "Whenever [subject] deals combat damage to [whose]": to a player, the one dealt it.
    DealsCombatDamageToPlayer,
    /// "At the beginning of [whose] [step]"
    /// ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)): "When a phase or
    /// step begins, any abilities that trigger 'at the beginning of' that phase or step
    /// trigger." The player is the one whose turn it is. No subject.
    Step(TurnStep),
    /// "At the beginning of [whose] main phase": either of a turn's two
    /// ([CR 505.1](https://yawgatog.com/resources/magic-rules/#R5051)): "There are two main
    /// phases in a turn." The first alone is [`TurnStep::PrecombatMain`]. No subject.
    MainPhase,
    /// "Whenever [whose] create one or more [subject] tokens": once per event, however many
    /// tokens it creates.
    CreatesTokens,
    /// "Whenever [whose] cast [subject]", a spell.
    Casts,
    /// "Whenever [whose] discard [subject]", a card: once for each card discarded
    /// ([CR 603.2c](https://yawgatog.com/resources/magic-rules/#R6032c)): "An ability triggers
    /// only once each time its trigger event occurs. However, it can trigger repeatedly if one
    /// event contains multiple occurrences." The discarding player is the card's owner
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    Discards,
    /// "Whenever [whose] draw a card": once for each card drawn (CR 603.2c), each draw one
    /// event of its own ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a
    /// player is instructed to draw multiple cards, that player performs that many individual
    /// card draws." A draw that a replacement effect replaced never happens
    /// ([CR 614.6](https://yawgatog.com/resources/magic-rules/#R6146)), so it triggers nothing.
    /// No subject.
    Draws,
    /// "Whenever [subject] becomes the target of a spell or ability" ([CR 603.2e]): "Some
    /// trigger events use the word 'becomes' ... These trigger only at the time the named
    /// event happens". It happens as a spell or ability is put on the stack with the object
    /// among its targets: a spell's targets are chosen as it is cast ([CR 115.1a]), an
    /// activated ability's as it is activated, and a triggered ability's as it is put on the
    /// stack. Once for each spell or ability, however many of its targets the object is. The
    /// player is the spell's or ability's controller.
    ///
    /// [CR 603.2e]: https://yawgatog.com/resources/magic-rules/#R6032e
    /// [CR 115.1a]: https://yawgatog.com/resources/magic-rules/#R1151a
    BecomesTarget,
    /// "Whenever [subject] mutates" ([CR 702.140d]): "An ability that triggers whenever a
    /// creature mutates triggers when a spell merges with a creature as a result of a
    /// resolving mutating creature spell." Every such ability of the merged permanent sees
    /// it, from its top card or from a card under it. The player is the spell's controller.
    ///
    /// [CR 702.140d]: https://yawgatog.com/resources/magic-rules/#R702140d
    Mutates,
    /// "When [condition]": a state trigger ([CR 603.8]): "Some triggered abilities trigger when
    /// a game state (such as a player controlling no permanents of a particular card type) is
    /// true, rather than triggering when an event occurs. These abilities trigger as soon as the
    /// game state matches the condition." The condition is read for the ability's controller and
    /// object, as an intervening "if" is, and only as it triggers: "A state-triggered ability
    /// doesn't trigger again until the ability has resolved, has been countered, or has
    /// otherwise left the stack." No subject.
    ///
    /// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
    State(&'static Condition),
    /// When an object deals damage of any kind to any recipient.
    DealsDamage,
    /// When a face-down permanent turns face up (see CR 708.8).
    TurnedFaceUp,
    /// A single chapter threshold or a consecutive range of thresholds for the same effect.
    /// Each threshold crossed upward by putting lore counters on the source triggers separately.
    Chapter { first: u8, last: u8 },
    /// "Whenever [subject] becomes the target of a spell." Like `BecomesTarget`, but the
    /// object the trigger records is the targeting spell and the player is its controller.
    BecomesTargetOfSpell,
    /// "At the beginning of [whose] first [step] each turn" (see [CR 500.6]).
    ///
    /// [CR 500.6]: https://yawgatog.com/resources/magic-rules/#R5006
    FirstStep(TurnStep),
    /// A permanent becomes a particular Class level (see CR 717.2a).
    ClassLevel(u8),
    /// "Whenever [whose] cast [subject] from [zone]", the zone it occupied before casting.
    CastsFrom(Zone),
}

impl TriggerEvent {
    /// Whether this trigger listens for a player finishing the cast of a spell.
    pub const fn is_cast(self) -> bool {
        matches!(self, Self::Casts | Self::CastsFrom(_))
    }

    /// Whether this cast trigger accepts a spell cast from `origin`.
    pub fn matches_cast_from(self, origin: Zone) -> bool {
        self == Self::Casts || matches!(self, Self::CastsFrom(zone) if zone == origin)
    }
}

impl PartialEq for TriggerEvent {
    fn eq(&self, other: &TriggerEvent) -> bool {
        match self {
            TriggerEvent::Chapter { first, last } => {
                matches!(other, TriggerEvent::Chapter { first: a, last: b } if first == a && last == b)
            }
            TriggerEvent::CastsFrom(a) => matches!(other, TriggerEvent::CastsFrom(b) if a == b),
            TriggerEvent::FirstStep(a) => matches!(other, TriggerEvent::FirstStep(b) if a == b),
            TriggerEvent::ClassLevel(a) => matches!(other, TriggerEvent::ClassLevel(b) if a == b),
            TriggerEvent::Step(a) => matches!(other, TriggerEvent::Step(b) if a == b),
            TriggerEvent::State(a) => {
                matches!(other, TriggerEvent::State(b) if std::ptr::eq(*a, *b))
            }
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Leaves
            | TriggerEvent::Attacks
            | TriggerEvent::PlayerAttacks
            | TriggerEvent::DealsCombatDamageToPlayer
            | TriggerEvent::MainPhase
            | TriggerEvent::CreatesTokens
            | TriggerEvent::Casts
            | TriggerEvent::Discards
            | TriggerEvent::Draws
            | TriggerEvent::BecomesTarget
            | TriggerEvent::BecomesTargetOfSpell
            | TriggerEvent::Mutates
            | TriggerEvent::DealsDamage
            | TriggerEvent::TurnedFaceUp => {
                std::mem::discriminant(self) == std::mem::discriminant(other)
            }
        }
    }
}

impl Eq for TriggerEvent {}

/// The entry, including step or chapter numbers where applicable. A state trigger's condition
/// is left out so its address cannot change the hash between runs.
impl std::hash::Hash for TriggerEvent {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            TriggerEvent::Step(step) | TriggerEvent::FirstStep(step) => step.hash(h),
            TriggerEvent::ClassLevel(level) => level.hash(h),
            TriggerEvent::CastsFrom(zone) => zone.hash(h),
            TriggerEvent::Chapter { first, last } => (first, last).hash(h),
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Leaves
            | TriggerEvent::Attacks
            | TriggerEvent::PlayerAttacks
            | TriggerEvent::DealsCombatDamageToPlayer
            | TriggerEvent::MainPhase
            | TriggerEvent::CreatesTokens
            | TriggerEvent::Casts
            | TriggerEvent::Discards
            | TriggerEvent::Draws
            | TriggerEvent::BecomesTarget
            | TriggerEvent::BecomesTargetOfSpell
            | TriggerEvent::Mutates
            | TriggerEvent::DealsDamage
            | TriggerEvent::TurnedFaceUp
            | TriggerEvent::State(_) => {}
        }
    }
}

/// The object a trigger's event is about.
#[derive(Clone, Copy, Debug)]
pub enum Subject {
    /// The object the ability is on: "this creature"
    /// ([CR 201.5](https://yawgatog.com/resources/magic-rules/#R2015)): "Text that refers to
    /// the object it's on by name means just that particular object".
    This,
    /// An object the filter accepts, read for the ability's controller: "a creature you
    /// control", "a noncreature spell". [`Filter::Any`] is any object, and what a trigger
    /// says of an event that is about none.
    A(Filter),
}

impl Trigger {
    /// "When this enters."
    pub const ENTERS: Trigger = Trigger::of(TriggerEvent::Enters, Subject::This);
    /// "When you cast this spell", which works on the stack ([`crate::Ability::on_the_stack`]).
    pub const CAST: Trigger = Trigger::of(TriggerEvent::Casts, Subject::This);
    /// "When this dies."
    pub const DIES: Trigger = Trigger::of(TriggerEvent::Dies, Subject::This);
    /// "When this leaves the battlefield."
    pub const LEAVES: Trigger = Trigger::of(TriggerEvent::Leaves, Subject::This);
    /// "Whenever this attacks."
    pub const ATTACKS: Trigger = Trigger::of(TriggerEvent::Attacks, Subject::This);
    /// "When this becomes the target of a spell or ability."
    pub const BECOMES_TARGET: Trigger = Trigger::of(TriggerEvent::BecomesTarget, Subject::This);
    /// "Whenever this becomes the target of a spell."
    pub const BECOMES_TARGET_OF_SPELL: Trigger =
        Trigger::of(TriggerEvent::BecomesTargetOfSpell, Subject::This);
    /// "Whenever this creature mutates."
    pub const MUTATES: Trigger = Trigger::of(TriggerEvent::Mutates, Subject::This);
    /// "Whenever this deals combat damage to a player."
    pub const DEALS_DAMAGE: Trigger = Trigger::of(TriggerEvent::DealsDamage, Subject::This);
    pub const TURNED_FACE_UP: Trigger = Trigger::of(TriggerEvent::TurnedFaceUp, Subject::This);

    pub const DEALS_COMBAT_DAMAGE_TO_A_PLAYER: Trigger =
        Trigger::of(TriggerEvent::DealsCombatDamageToPlayer, Subject::This);

    /// `event` happening to `subject`, whoever the player.
    pub const fn of(event: TriggerEvent, subject: Subject) -> Trigger {
        Trigger {
            event,
            subject,
            whose: Players::EachPlayer,
            delayed: false,
            exact_turn: false,
        }
    }

    /// "When lore put on this Saga crosses any chapter from `first` through `last`."
    pub(crate) const fn chapters(first: u8, last: u8) -> Trigger {
        Trigger::of(TriggerEvent::Chapter { first, last }, Subject::This)
    }

    /// "At the beginning of [whose] [step]."
    pub const fn step(step: TurnStep, whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Step(step), Subject::A(Filter::Any)).whose(whose)
    }

    /// "At the beginning of [whose] first [step] each turn." A skipped step never begins
    /// and does not trigger the ability (see [CR 614.10a]).
    ///
    /// [CR 614.10a]: https://yawgatog.com/resources/magic-rules/#R61410a
    pub const fn first_step(step: TurnStep, whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::FirstStep(step), Subject::A(Filter::Any)).whose(whose)
    }

    /// "At the beginning of [whose] main phase."
    pub const fn main_phase(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::MainPhase, Subject::A(Filter::Any)).whose(whose)
    }

    /// "Whenever [whose] attack."
    pub const fn attack(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::PlayerAttacks, Subject::A(Filter::Any)).whose(whose)
    }

    /// "Whenever [whose] create one or more tokens" that `tokens` accepts.
    pub const fn creates_tokens(whose: Players, tokens: Filter) -> Trigger {
        Trigger::of(TriggerEvent::CreatesTokens, Subject::A(tokens)).whose(whose)
    }

    /// "Whenever [whose] cast a spell" that `spell` accepts.
    pub const fn casts(whose: Players, spell: Filter) -> Trigger {
        Trigger::of(TriggerEvent::Casts, Subject::A(spell)).whose(whose)
    }

    /// "Whenever [whose] cast a spell" that `spell` accepts, from `zone`.
    pub const fn casts_from(whose: Players, spell: Filter, zone: Zone) -> Trigger {
        Trigger::of(TriggerEvent::CastsFrom(zone), Subject::A(spell)).whose(whose)
    }

    /// "Whenever [whose] discard a card."
    pub const fn discards(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Discards, Subject::A(Filter::Any)).whose(whose)
    }

    /// "Whenever [whose] draw a card."
    pub const fn draws(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Draws, Subject::A(Filter::Any)).whose(whose)
    }

    /// "When [condition]", a state trigger (CR 603.8).
    pub const fn state(condition: &'static Condition) -> Trigger {
        Trigger::of(TriggerEvent::State(condition), Subject::A(Filter::Any))
    }

    /// This trigger, for these players.
    #[must_use]
    pub const fn whose(self, whose: Players) -> Trigger {
        Trigger { whose, ..self }
    }

    /// This trigger as a delayed one (CR 603.7), "the next time": "at the beginning of the
    /// next end step" is `Trigger::step(TurnStep::End, Players::EachPlayer).next()`.
    #[must_use]
    pub const fn next(self) -> Trigger {
        Trigger {
            delayed: true,
            ..self
        }
    }

    /// "At the beginning of that turn's [step]": a delayed ability scheduled with an
    /// extra turn, expiring with it if that step never begins (CR 603.7b and CR 614.10a).
    pub const fn during_extra_turn(step: TurnStep) -> Trigger {
        Trigger {
            delayed: true,
            exact_turn: true,
            ..Trigger::step(step, Players::EachPlayer)
        }
    }

    /// Whether this is `event` happening to the object the ability is on, as
    /// [`Trigger::ENTERS`] is of [`TriggerEvent::Enters`].
    pub fn is_this(&self, event: TriggerEvent) -> bool {
        self.event == event && matches!(self.subject, Subject::This)
    }
}
