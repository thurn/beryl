//! Listeners: a game's triggered abilities, each under the kind of event it listens to
//! (architecture §6.4), and the triggers the engine has no code to match.
//!
//! A [`Trigger`] is a product of an event, a subject and players, and says more than the
//! engine matches. What it matches is said here, in one place: [`listens_to`] gives a
//! trigger's kind or refuses it, and a game's card table is built only from triggers it
//! gives a kind ([`crate::table::TableError::Trigger`]). The rules' matcher then reads a
//! subject with no case for what was refused, and the players through
//! [`crate::filter::is_for`].

use beryl_card::{
    Ability, AbilityIx, AbilityKind, CardDef, FaceIx, Filter, Players, Subject, Trigger,
    TriggerEvent, Zone, ZoneSet,
};

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
    /// The zones its ability works in ([`Ability::zones`]).
    pub zones: ZoneSet,
}

/// The zones other than the battlefield in which the engine matches a triggered ability: the
/// public ones an object of a player is in, where its card is known
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Graveyard, battlefield,
/// stack, exile, ante, and command are public zones." A card in a library or a hand may yet
/// be dealt anew among its owner's hidden cards, so no ability is matched there.
pub const AWAY: ZoneSet = ZoneSet::GRAVEYARD
    .union(ZoneSet::of(Zone::Exile))
    .union(ZoneSet::STACK);

/// Why the engine cannot match a trigger: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmatched {
    /// Its event is not matched against that subject: an object other than the one the
    /// ability is on entering, attacking or dealing damage, the object itself being cast or
    /// discarded, or a subject for an event that is about no object.
    Subject,
    /// Its event is not matched for those players: any but "you", "each opponent" and "each
    /// player", or any but "each player" for an event no player does.
    Whose,
    /// It is delayed and waits for an event other than the beginning of a step or phase.
    Delayed,
    /// It works in a zone the engine matches no ability in ([`AWAY`]): a library or a hand.
    Zone,
}

/// What a kind of event is about, as the engine matches it.
enum About {
    /// One object, matched only as the object the ability is on.
    This,
    /// One object or a set of them, matched by a filter.
    Filtered,
    /// One object, matched either way: an ability that looks back in time at the
    /// permanents leaving the battlefield sees each of them, its own permanent among them.
    Either,
    /// No object.
    Nothing,
}

/// The kind of event a trigger listens to, or why the engine cannot match it. A delayed trigger
/// listens to none (`None`): it waits in the state, not in the table, for the beginning of a step
/// ([CR 603.7]): "An effect may create a delayed triggered ability that can do something at a later
/// time."
///
/// [CR 603.7]: https://yawgatog.com/resources/magic-rules/#R6037
///
/// A spell's own "when you cast this spell" is matched where the ability works on the stack
/// ([CR 113.6k]): "A trigger condition that can't trigger from the battlefield functions in all
/// zones it can trigger from."
///
/// [CR 113.6k]: https://yawgatog.com/resources/magic-rules/#R1136k
pub fn listens_to(trigger: &Trigger, zones: ZoneSet) -> Result<Option<EventKind>, Unmatched> {
    let (kind, about, has_player) = match trigger.event {
        TriggerEvent::Enters => (EventKind::Enters, About::This, false),
        TriggerEvent::Dies => (EventKind::Dies, About::Either, false),
        TriggerEvent::Leaves => (EventKind::Leaves, About::Either, false),
        TriggerEvent::Attacks => (EventKind::Attack, About::This, false),
        TriggerEvent::PlayerAttacks => (EventKind::Attack, About::Filtered, true),
        TriggerEvent::DealsCombatDamageToPlayer => (EventKind::Damage, About::This, true),
        TriggerEvent::Step(_) | TriggerEvent::MainPhase => (EventKind::Step, About::Nothing, true),
        TriggerEvent::CreatesTokens => (EventKind::Tokens, About::Filtered, true),
        TriggerEvent::Casts => (EventKind::Cast, About::Filtered, true),
        TriggerEvent::Discards => (EventKind::Discard, About::Filtered, true),
        TriggerEvent::Draws => (EventKind::Draw, About::Nothing, true),
        TriggerEvent::State(_) => (EventKind::State, About::Nothing, false),
    };
    if !AWAY.union(ZoneSet::BATTLEFIELD).includes(zones) {
        return Err(Unmatched::Zone);
    }
    let cast_on_the_stack = trigger.event == TriggerEvent::Casts && zones.contains(Zone::Stack);
    let subject = match (about, trigger.subject) {
        (About::Filtered, Subject::This) if cast_on_the_stack => Ok(()),
        (About::This | About::Either, Subject::This)
        | (About::Filtered | About::Either, Subject::A(_))
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
            if let Some(kind) = listens_to(trigger, ability.zones())? {
                let index = u8::try_from(index).expect("a face of at most 64 abilities");
                listeners.push(Listener {
                    kind,
                    face,
                    ability: AbilityIx::new(index),
                    trigger,
                    zones: ability.zones(),
                });
            }
        }
    }
    Ok(listeners)
}

/// Whether the engine finds `ability` wherever it says it works ([CR 113.6b]): "An ability
/// that states which zones it functions in functions only from those zones." A triggered
/// ability is matched on the battlefield and in the zones of [`AWAY`] ([`listens_to`]). Any
/// other kind is found only where its kind works: activated and static abilities are read of
/// permanents, and a spell's of the spell.
///
/// [CR 113.6b]: https://yawgatog.com/resources/magic-rules/#R1136b
pub fn found_where_it_works(ability: &Ability) -> bool {
    ability.kind() == AbilityKind::Triggered || ability.works_where_its_kind_does()
}

#[cfg(test)]
mod tests {
    use beryl_card::{Objects, TurnStep, Types};

    use super::*;

    const B: ZoneSet = ZoneSet::BATTLEFIELD;

    #[test]
    fn a_trigger_listens_to_the_kind_of_its_event() {
        let creature = Filter::Types(Types::CREATURE);
        for (trigger, kind) in [
            (Trigger::ENTERS, EventKind::Enters),
            (Trigger::DIES, EventKind::Dies),
            (
                Trigger::of(TriggerEvent::Dies, Subject::A(creature)),
                EventKind::Dies,
            ),
            (
                Trigger::of(TriggerEvent::Leaves, Subject::This),
                EventKind::Leaves,
            ),
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
            (
                Trigger::state(&beryl_card::Condition::WonFlip),
                EventKind::State,
            ),
        ] {
            assert_eq!(listens_to(&trigger, B), Ok(Some(kind)), "{trigger:?}");
        }
    }

    /// A delayed trigger is in no index: it waits in the state for its step.
    #[test]
    fn a_delayed_trigger_waits_for_a_step_and_listens_to_nothing() {
        let end = Trigger::step(TurnStep::End, Players::EachPlayer).next();
        assert_eq!(listens_to(&end, B), Ok(None));
        assert_eq!(
            listens_to(&Trigger::main_phase(Players::You).next(), B),
            Ok(None)
        );
        assert_eq!(
            listens_to(&Trigger::draws(Players::You).next(), B),
            Err(Unmatched::Delayed)
        );
        assert_eq!(
            listens_to(&Trigger::ENTERS.next(), B),
            Err(Unmatched::Delayed)
        );
    }

    #[test]
    fn a_subject_the_engine_does_not_match_is_refused() {
        let creature = Subject::A(Filter::Types(Types::CREATURE));
        for trigger in [
            // Another object entering, attacking or dealing damage.
            Trigger::of(TriggerEvent::Enters, creature),
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
            assert_eq!(
                listens_to(&trigger, B),
                Err(Unmatched::Subject),
                "{trigger:?}"
            );
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
            assert_eq!(
                listens_to(&trigger, B),
                Err(Unmatched::Whose),
                "{trigger:?}"
            );
        }
    }

    /// A spell's own cast is matched only where its ability works on the stack, and nothing
    /// is matched in a hidden zone.
    #[test]
    fn where_an_ability_works_decides_what_is_matched() {
        let cast_this = Trigger::of(TriggerEvent::Casts, Subject::This);
        assert_eq!(listens_to(&cast_this, B), Err(Unmatched::Subject));
        assert_eq!(
            listens_to(&cast_this, ZoneSet::STACK),
            Ok(Some(EventKind::Cast))
        );
        let draws = Trigger::draws(Players::You);
        assert_eq!(
            listens_to(&draws, ZoneSet::GRAVEYARD),
            Ok(Some(EventKind::Draw))
        );
        assert_eq!(listens_to(&draws, ZoneSet::ALL), Err(Unmatched::Zone));
    }
}
