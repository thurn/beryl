//! Listeners: a game's triggered abilities, each under the kind of event it listens to
//! (architecture §6.4), and the triggers the engine has no code to match.
//!
//! A [`Trigger`] is a product of an event, a subject and players, and says more than the
//! engine matches. What it matches is said here, in one place: [`listens_to`] gives a
//! trigger's kind or refuses it, and a game's card table is built only from triggers it
//! gives a kind ([`crate::table::TableError::Trigger`]). The rules' matcher then reads a
//! subject with no case for what was refused, and the players through [`is_for`].

use beryl_card::{AbilityIx, CardDef, FaceIx, Filter, Players, Subject, Trigger, TriggerEvent};

use crate::event::kind;
use crate::filter::is_an_opponent;
use crate::ids::Seat;

/// One triggered ability of a card of the game, under the kind of event it listens to.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    /// The one bit of [`kind`] its event has.
    pub kind: u16,
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

/// The kind of event a trigger listens to, one bit of [`kind`], or why the engine cannot
/// match it. A delayed trigger listens to none: it waits in the state, not in the table,
/// for the beginning of a step
/// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)): "An effect may create a
/// delayed triggered ability that can do something at a later time."
pub fn listens_to(trigger: &Trigger) -> Result<u16, Unmatched> {
    let (kind, about, has_player) = match trigger.event {
        TriggerEvent::Enters => (kind::ENTERS, About::This, false),
        TriggerEvent::Dies => (kind::DIES, About::This, false),
        TriggerEvent::Attacks => (kind::ATTACK, About::This, false),
        TriggerEvent::PlayerAttacks => (kind::ATTACK, About::Filtered, true),
        TriggerEvent::DealsCombatDamageToPlayer => (kind::DAMAGE, About::This, true),
        TriggerEvent::Step(_) | TriggerEvent::MainPhase => (kind::STEP, About::Nothing, true),
        TriggerEvent::CreatesTokens => (kind::TOKENS, About::Filtered, true),
        TriggerEvent::Casts => (kind::CAST, About::Filtered, true),
        TriggerEvent::Discards => (kind::DISCARD, About::Filtered, true),
        TriggerEvent::Draws => (kind::DRAW, About::Nothing, true),
    };
    let subject = match (about, trigger.subject) {
        (About::This, Subject::This)
        | (About::Filtered, Subject::A(_))
        | (About::Nothing, Subject::A(Filter::Any)) => Ok(()),
        _ => Err(Unmatched::Subject),
    };
    let whose = match trigger.whose {
        Players::EachPlayer => Ok(()),
        Players::You | Players::EachOpponent if has_player => Ok(()),
        _ => Err(Unmatched::Whose),
    };
    subject.and(whose)?;
    match (trigger.delayed, kind) {
        (false, kind) => Ok(kind),
        (true, kind::STEP) => Ok(0),
        (true, _) => Err(Unmatched::Delayed),
    }
}

/// Whether `player` is one of the players a trigger is for, read for `you`, its ability's
/// controller ([`Trigger::whose`]).
///
/// # Panics
///
/// For players [`listens_to`] refuses, which no trigger of a game's table has.
#[inline]
pub fn is_for(whose: Players, player: Seat, you: Seat) -> bool {
    match whose {
        // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
        // controller".
        Players::You => player == you,
        Players::EachOpponent => is_an_opponent(player, you),
        Players::EachPlayer => true,
        _ => unreachable!("the card table refuses a trigger for other players"),
    }
}

/// The listeners of a card's faces, in the order the faces list their abilities, and the
/// first trigger of it the engine cannot match.
pub(crate) fn of(def: &CardDef) -> Result<Vec<Listener>, Unmatched> {
    let mut listeners = Vec::new();
    for (face, at) in def.face_ixs().zip(def.faces) {
        for (index, ability) in at.abilities.iter().enumerate() {
            let Some(trigger) = ability.trigger else {
                continue;
            };
            let kind = listens_to(trigger)?;
            if kind != 0 {
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
            (Trigger::ENTERS, kind::ENTERS),
            (Trigger::DIES, kind::DIES),
            (Trigger::ATTACKS, kind::ATTACK),
            (Trigger::attack(Players::EachOpponent), kind::ATTACK),
            (Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER, kind::DAMAGE),
            (
                Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER.whose(Players::EachOpponent),
                kind::DAMAGE,
            ),
            (
                Trigger::step(TurnStep::Upkeep, Players::EachPlayer),
                kind::STEP,
            ),
            (Trigger::main_phase(Players::You), kind::STEP),
            (
                Trigger::creates_tokens(Players::You, creature),
                kind::TOKENS,
            ),
            (Trigger::casts(Players::EachOpponent, creature), kind::CAST),
            (Trigger::discards(Players::You), kind::DISCARD),
            (Trigger::draws(Players::EachPlayer), kind::DRAW),
        ] {
            assert_eq!(listens_to(&trigger), Ok(kind), "{trigger:?}");
        }
    }

    /// A delayed trigger is in no index: it waits in the state for its step.
    #[test]
    fn a_delayed_trigger_waits_for_a_step_and_listens_to_nothing() {
        let end = Trigger::step(TurnStep::End, Players::EachPlayer).next();
        assert_eq!(listens_to(&end), Ok(0));
        assert_eq!(listens_to(&Trigger::main_phase(Players::You).next()), Ok(0));
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

    #[test]
    fn a_trigger_is_for_the_players_it_names() {
        let (you, other) = (Seat::BOTH[0], Seat::BOTH[1]);
        assert!(is_for(Players::You, you, you));
        assert!(!is_for(Players::You, other, you));
        assert!(is_for(Players::EachOpponent, other, you));
        assert!(!is_for(Players::EachOpponent, you, you));
        assert!(is_for(Players::EachPlayer, you, you));
        assert!(is_for(Players::EachPlayer, other, you));
    }
}
