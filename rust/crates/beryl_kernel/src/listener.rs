//! Listeners: a game's triggered abilities, each under the kind of event it listens to
//! (architecture §6.4), and the triggers the engine has no code to match.
//!
//! A [`Trigger`] is a product of an event, a subject and players, and says more than the
//! engine matches. What it matches is said here, in one place: [`listens_to`] gives a
//! trigger's kind or refuses it, and a game's card table is built only from triggers it
//! gives a kind ([`crate::table::TableError::Trigger`]). The rules' matcher then reads a
//! subject with no case for what was refused, and the players through
//! [`crate::filter::is_for`].

use beryl_card::{AbilityIx, CardDef, FaceIx, Filter, Players, Subject, Trigger, TriggerEvent};

use crate::event::EventKind;
use crate::filter::reads;

/// One triggered ability of a card of the game, under the kind of event it listens to.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    /// The kind of event it listens to.
    pub kind: EventKind,
    pub face: FaceIx,
    pub ability: AbilityIx,
    pub trigger: &'static Trigger,
}

/// Why the engine cannot match a trigger: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmatched {
    /// Its event is not matched against that subject: an object other than the one the
    /// ability is on entering, dying, attacking or dealing damage, the object itself being
    /// cast or discarded, or a subject for an event that is about no object.
    Subject,
    /// Its event is not matched for those players: any but "you", "each opponent" and "each
    /// player", or any but "each player" for an event no player does.
    Whose,
    /// It is delayed and waits for an event other than the beginning of a step or phase.
    Delayed,
}

/// What a kind of event is about, as the engine matches it.
enum About {
    /// One object, matched only as the object the ability is on.
    This,
    /// One object or a set of them, matched by a filter.
    Filtered,
    /// No object.
    Nothing,
}

/// The kind of event a trigger listens to, or why the engine cannot match it. A delayed trigger
/// listens to none (`None`): it waits in the state, not in the table, for the beginning of a step
/// ([CR 603.7]): "An effect may create a delayed triggered ability that can do something at a later
/// time."
///
/// [CR 603.7]: https://yawgatog.com/resources/magic-rules/#R6037
pub fn listens_to(trigger: &Trigger) -> Result<Option<EventKind>, Unmatched> {
    let (kind, about, has_player) = match trigger.event {
        TriggerEvent::Enters => (EventKind::Enters, About::This, false),
        TriggerEvent::Dies => (EventKind::Dies, About::This, false),
        TriggerEvent::Attacks => (EventKind::Attack, About::This, false),
        TriggerEvent::PlayerAttacks => (EventKind::Attack, About::Filtered, true),
        TriggerEvent::DealsCombatDamageToPlayer => (EventKind::Damage, About::This, true),
        TriggerEvent::Step(_) | TriggerEvent::MainPhase => (EventKind::Step, About::Nothing, true),
        TriggerEvent::CreatesTokens => (EventKind::Tokens, About::Filtered, true),
        TriggerEvent::Casts => (EventKind::Cast, About::Filtered, true),
        TriggerEvent::Discards => (EventKind::Discard, About::Filtered, true),
        TriggerEvent::Draws => (EventKind::Draw, About::Nothing, true),
    };
    let subject = match (about, trigger.subject) {
        (About::This, Subject::This)
        | (About::Filtered, Subject::A(_))
        | (About::Nothing, Subject::A(Filter::Any)) => Ok(()),
        _ => Err(Unmatched::Subject),
    };
    let whose = match trigger.whose {
        Players::EachPlayer => Ok(()),
        ref players if has_player && reads(players) => Ok(()),
        _ => Err(Unmatched::Whose),
    };
    subject.and(whose)?;
    match (trigger.delayed, kind) {
        (false, kind) => Ok(Some(kind)),
        (true, EventKind::Step) => Ok(None),
        (true, _) => Err(Unmatched::Delayed),
    }
}

/// The listeners of a card's faces, in the order the faces list their abilities, and the
/// first trigger of it the engine cannot match.
pub(crate) fn of(def: &CardDef) -> Result<Vec<Listener>, Unmatched> {
    let mut listeners = Vec::new();
    for (face, at) in def.face_ixs().zip(def.faces) {
        for (index, ability) in at.abilities.iter().enumerate() {
            let Some(trigger) = ability.trigger() else {
                continue;
            };
            if let Some(kind) = listens_to(trigger)? {
                let index = u8::try_from(index).expect("a face of at most 64 abilities");
                listeners.push(Listener {
                    kind,
                    face,
                    ability: AbilityIx::new(index),
                    trigger,
                });
            }
        }
    }
    Ok(listeners)
}

#[cfg(test)]
mod tests {
    use beryl_card::{Objects, TurnStep, Types};

    use super::*;

    #[test]
    fn a_trigger_listens_to_the_kind_of_its_event() {
        let creature = Filter::Types(Types::CREATURE);
        for (trigger, kind) in [
            (Trigger::ENTERS, EventKind::Enters),
            (Trigger::DIES, EventKind::Dies),
            (Trigger::ATTACKS, EventKind::Attack),
            (Trigger::attack(Players::EachOpponent), EventKind::Attack),
            (Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER, EventKind::Damage),
            (
                Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER.whose(Players::EachOpponent),
                EventKind::Damage,
            ),
            (
                Trigger::step(TurnStep::Upkeep, Players::EachPlayer),
                EventKind::Step,
            ),
            (Trigger::main_phase(Players::You), EventKind::Step),
            (
                Trigger::creates_tokens(Players::You, creature),
                EventKind::Tokens,
            ),
            (
                Trigger::casts(Players::EachOpponent, creature),
                EventKind::Cast,
            ),
            (Trigger::discards(Players::You), EventKind::Discard),
            (Trigger::draws(Players::EachPlayer), EventKind::Draw),
        ] {
            assert_eq!(listens_to(&trigger), Ok(Some(kind)), "{trigger:?}");
        }
    }

    /// A delayed trigger is in no index: it waits in the state for its step.
    #[test]
    fn a_delayed_trigger_waits_for_a_step_and_listens_to_nothing() {
        let end = Trigger::step(TurnStep::End, Players::EachPlayer).next();
        assert_eq!(listens_to(&end), Ok(None));
        assert_eq!(
            listens_to(&Trigger::main_phase(Players::You).next()),
            Ok(None)
        );
        assert_eq!(
            listens_to(&Trigger::draws(Players::You).next()),
            Err(Unmatched::Delayed)
        );
        assert_eq!(listens_to(&Trigger::ENTERS.next()), Err(Unmatched::Delayed));
    }

    #[test]
    fn a_subject_the_engine_does_not_match_is_refused() {
        let creature = Subject::A(Filter::Types(Types::CREATURE));
        for trigger in [
            // Another object entering, dying, attacking or dealing damage.
            Trigger::of(TriggerEvent::Enters, creature),
            Trigger::of(TriggerEvent::Dies, creature),
            Trigger::of(TriggerEvent::Attacks, creature),
            Trigger::of(TriggerEvent::DealsCombatDamageToPlayer, creature),
            // The object itself where the event is matched by a filter.
            Trigger::of(TriggerEvent::Casts, Subject::This),
            Trigger::of(TriggerEvent::Discards, Subject::This),
            Trigger::of(TriggerEvent::CreatesTokens, Subject::This),
            Trigger::of(TriggerEvent::PlayerAttacks, Subject::This),
            // A subject where the event is about no object.
            Trigger::of(TriggerEvent::Step(TurnStep::Upkeep), creature),
            Trigger::of(TriggerEvent::MainPhase, Subject::This),
            Trigger::of(TriggerEvent::Draws, creature),
        ] {
            assert_eq!(listens_to(&trigger), Err(Unmatched::Subject), "{trigger:?}");
        }
    }

    #[test]
    fn players_the_engine_does_not_match_are_refused() {
        for trigger in [
            // An event no player does.
            Trigger::ENTERS.whose(Players::You),
            Trigger::DIES.whose(Players::EachOpponent),
            Trigger::ATTACKS.whose(Players::You),
            // Players a trigger event cannot be read for.
            Trigger::casts(Players::ThatPlayer, Filter::Any),
            Trigger::draws(Players::ControllerOf(Objects::This)),
            Trigger::step(TurnStep::Upkeep, Players::OwnerOf(Objects::This)),
        ] {
            assert_eq!(listens_to(&trigger), Err(Unmatched::Whose), "{trigger:?}");
        }
    }
}
