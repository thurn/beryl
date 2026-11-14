//! Triggers: what a triggered ability triggers on, said as an event, the object it is about
//! and the player it happens to or who does it.

use crate::effect::Players;
use crate::filter::Filter;
use crate::ids::TurnStep;

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
}

/// The event of a trigger: one of the kinds of event the engine raises. An entry names what
/// happens and never which object or player it happens to ([`Subject`], [`Players`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// "When this dies."
    pub const DIES: Trigger = Trigger::of(TriggerEvent::Dies, Subject::This);
    /// "Whenever this attacks."
    pub const ATTACKS: Trigger = Trigger::of(TriggerEvent::Attacks, Subject::This);
    /// "Whenever this deals combat damage to a player."
    pub const DEALS_COMBAT_DAMAGE_TO_A_PLAYER: Trigger =
        Trigger::of(TriggerEvent::DealsCombatDamageToPlayer, Subject::This);

    /// `event` happening to `subject`, whoever the player.
    pub const fn of(event: TriggerEvent, subject: Subject) -> Trigger {
        Trigger {
            event,
            subject,
            whose: Players::EachPlayer,
            delayed: false,
        }
    }

    /// "At the beginning of [whose] [step]."
    pub const fn step(step: TurnStep, whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Step(step), Subject::A(Filter::Any)).whose(whose)
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

    /// "Whenever [whose] discard a card."
    pub const fn discards(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Discards, Subject::A(Filter::Any)).whose(whose)
    }

    /// "Whenever [whose] draw a card."
    pub const fn draws(whose: Players) -> Trigger {
        Trigger::of(TriggerEvent::Draws, Subject::A(Filter::Any)).whose(whose)
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

    /// Whether this is `event` happening to the object the ability is on, as
    /// [`Trigger::ENTERS`] is of [`TriggerEvent::Enters`].
    pub fn is_this(&self, event: TriggerEvent) -> bool {
        self.event == event && matches!(self.subject, Subject::This)
    }
}
