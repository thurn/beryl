//! State-triggered abilities: matching them after the board changes (CR 603.8).
//!
//! Implements: none. Matches state triggers under the rules declared by its parent module.

use super::{
    AbilityController, AbilityIx, AbilityRef, Capability, EventKind, FaceIx, Full, GameTable,
    Layout, PendingTrigger, Queue, State, Trigger, TriggerEvent, TriggerInfo, TriggeredAbility,
    Words, away, away_controllers, enqueue, of_object, waiting_or_on_stack,
};

/// Triggers each state-triggered ability whose condition holds now and that is neither waiting to
/// be put on the stack nor on it ([CR 603.8]): "These abilities trigger as soon as the game state
/// matches the condition. ... A state-triggered ability doesn't trigger again until the ability has
/// resolved, has been countered, or has otherwise left the stack. Then, if the object with the
/// ability is still in the same zone and the game state still matches its trigger condition, the
/// ability will trigger again." Whether one is waiting or on the stack is found there, by its source
/// and ability, so nothing else records it; a source that changed zones is a new object, whose
/// ability is another.
///
/// Checked as state-based actions are, behind the game's [`EventKind::State`] listeners: a game
/// none of whose cards has a state trigger pays one AND, and the rest is out of line.
///
/// The permanents' state triggers are read only when they are due ([`State::states_due`]): after
/// the layer results were stored, or while one of them waits or is on the stack. A condition the
/// kernel calls [`watched`] reads only what marks the layer results stale when it changes, so it
/// cannot begin to hold between two recomputes. One that waits or is on the stack keeps them due,
/// because leaving the stack marks nothing stale and the check after it must trigger the ability
/// again if its condition still holds. A game with a state trigger that is not watched
/// ([`Capability::UnwatchedStateTrigger`]) reads them at every check. In debug builds, a check
/// that skips them asserts that none would have triggered.
///
/// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
/// [`watched`]: beryl_kernel::static_ability::watched
#[inline]
pub(crate) fn match_states<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    if t.listens_anywhere().hears(EventKind::State) {
        match_states_out_of_line(s, t)?;
    }
    Ok(())
}

#[cold]
#[inline(never)]
fn match_states_out_of_line<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    let kind = EventKind::State;
    if t.listens_any().hears(kind) && s.listening().unwrap_or(t.listens_any()).hears(kind) {
        if s.states_due() || t.can(Capability::UnwatchedStateTrigger) {
            let mut pending = false;
            let mut known = Known::default();
            for i in 0..s.battlefield().len() {
                let slot = s.battlefield()[i];
                let what = s.definition(slot);
                if !what.may_listen(t).hears(kind) {
                    continue;
                }
                let (card, face) = what.record(t);
                for l in t.listeners(card, face, kind) {
                    if s.has_ability(slot, l.ability) {
                        pending |= state_trigger(
                            s, t, &mut known, slot, card, face, l.ability, l.trigger,
                        )?;
                    }
                }
            }
            s.set_states_due(pending);
        } else {
            #[cfg(debug_assertions)]
            none_would_trigger(s, t);
        }
    }
    if t.listens_away().hears(kind) {
        for &slot in s.in_game_order(t, away(s, t))?.iter() {
            let zone = s.object(slot).zone();
            let (card, face) = s.definition(slot).record(t);
            for controller in away_controllers(s, slot).in_seat_order() {
                for l in t.away_listeners(card, face, kind) {
                    if l.zones.contains(zone) && s.has_ability(slot, l.ability) {
                        let ability = TriggeredAbility::own(l.ability);
                        let of = AbilityRef {
                            controller: controller.into(),
                            ..of_object(s, slot, card, face, ability)
                        };
                        state_trigger_of(s, t, of, l.trigger, None)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Asserts that no state trigger on the battlefield would trigger now: [`match_states`] skipped
/// them because nothing marked the layer results stale and none was waiting or on the stack, so
/// a condition that holds now began to hold through a change the kernel does not watch
/// ([`beryl_kernel::static_ability::watched`], [`beryl_kernel::static_ability::Inputs`]).
#[cfg(debug_assertions)]
fn none_would_trigger<L: Layout>(s: &State<L>, t: &GameTable) {
    let kind = EventKind::State;
    for &slot in s.battlefield() {
        let what = s.definition(slot);
        if !what.may_listen(t).hears(kind) {
            continue;
        }
        let (card, face) = what.record(t);
        for l in t.listeners(card, face, kind) {
            if s.has_ability(slot, l.ability) {
                let of = of_object(s, slot, card, face, TriggeredAbility::own(l.ability));
                assert!(
                    matches!(due(s, t, of, l.trigger, &mut None), Due::No),
                    "a state trigger began to hold with nothing marking the layer results stale"
                );
            }
        }
    }
}

/// The answers to [`shared`] state-trigger conditions already found in one call of
/// [`match_states`]: the card, face, ability and controller they were found for, and whether
/// the condition holds. It has room for four and does not allocate; once full, later
/// permanents are evaluated without being stored.
#[derive(Default)]
struct Known<L: Layout>([Option<Answer<L>>; 4]);

/// A [`Known`] entry: card, face, ability, controller, and whether the condition holds.
type Answer<L> = (
    <L as Layout>::Card,
    FaceIx,
    AbilityIx,
    AbilityController,
    bool,
);

/// Triggers the state-triggered ability `ability` of the object in `slot`, if its condition
/// holds and it is neither waiting nor on the stack ([`match_states`]). Returns whether it is
/// now waiting or on the stack.
///
/// Permanents with the same card, face, ability and controller get the same answer from a
/// [`shared`] condition, so it is read once and kept in `known`, unless a text change is in
/// force on the permanent: it may change which land type the condition names, so such a
/// permanent is always evaluated on its own and its answer is never stored. Whether the
/// ability is waiting or on the stack is still asked for each permanent.
#[allow(
    clippy::too_many_arguments,
    reason = "the key of the answer, spelled out"
)]
fn state_trigger<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    known: &mut Known<L>,
    slot: L::Slot,
    card: L::Card,
    face: FaceIx,
    ability: AbilityIx,
    trigger: &Trigger,
) -> Result<bool, Full> {
    let of = of_object(s, slot, card, face, TriggeredAbility::own(ability));
    let TriggerEvent::State(condition) = trigger.event else {
        unreachable!("a listener of state triggers is a state trigger");
    };
    if !beryl_kernel::static_ability::shared(condition) || s.words_of(slot) != Words::NONE {
        return state_trigger_of(s, t, of, trigger, None);
    }
    let key = (card, face, ability, of.controller);
    let at = known
        .0
        .iter()
        .flatten()
        .position(|k| key == (k.0, k.1, k.2, k.3));
    let answer = at.and_then(|i| known.0[i].map(|k| k.4));
    let mut asked = answer;
    let result = state_trigger_of(s, t, of, trigger, Some(&mut asked));
    if let (None, Some(holds)) = (answer, asked)
        && let Some(room) = known.0.iter_mut().find(|k| k.is_none())
    {
        *room = Some((key.0, key.1, key.2, key.3, holds));
    }
    result
}

/// [`state_trigger`] for the ability `of`, whose controller the caller chose: one of the
/// players of [`away_controllers`]. Each player's instance of a card's ability in a shared
/// zone waits or is on the stack apart from the other's. Returns whether it is now waiting or
/// on the stack, or may be: an intervening "if" may still have refused it.
fn state_trigger_of<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    of: AbilityRef<L, TriggeredAbility>,
    trigger: &Trigger,
    known: Option<&mut Option<bool>>,
) -> Result<bool, Full> {
    match due(s, t, of, trigger, known.unwrap_or(&mut None)) {
        Due::Pending => Ok(true),
        Due::Holds(pending) => enqueue(s, t, pending).map(|()| true),
        Due::No => Ok(false),
    }
}

/// Where a state-triggered ability stands at a check ([`match_states`]).
enum Due<L: Layout> {
    /// It is waiting or on the stack, so it does not trigger again (CR 603.8).
    Pending,
    /// Its condition holds and it is neither waiting nor on the stack: this triggers.
    Holds(PendingTrigger<L>),
    /// Its condition does not hold.
    No,
}

/// Where the state-triggered ability `of`, whose trigger is `trigger`, stands now. A
/// `Some` in `answer` is whether the condition holds, already known; a `None` is replaced
/// by the answer if the condition is read.
fn due<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    of: AbilityRef<L, TriggeredAbility>,
    trigger: &Trigger,
    answer: &mut Option<bool>,
) -> Due<L> {
    let TriggerEvent::State(condition) = trigger.event else {
        unreachable!("a listener of state triggers is a state trigger");
    };
    let same = |other: &AbilityRef<L, TriggeredAbility>| {
        (
            other.source,
            other.card,
            other.face,
            other.ability,
            other.controller,
        ) == (of.source, of.card, of.face, of.ability, of.controller)
    };
    if waiting_or_on_stack(s, same) {
        return Due::Pending;
    }
    let info = TriggerInfo {
        object: None,
        player: of.controller.seat(),
        amount: 0,
    };
    let pending = PendingTrigger { of, info };
    let holds = *answer.get_or_insert_with(|| crate::interpret::holds(s, t, &pending, condition));
    if holds { Due::Holds(pending) } else { Due::No }
}
