//! What a script's shape still reads out of the order of a view's events and of the seat's own
//! prompt: which events happen together, the order two kinds of event are scripted in, and
//! what the viewer was already looking at. What an event came from, what mana pays for and
//! which entry of the stack an event is about are the engine's to say (`hear.rs`, plan K23),
//! and nothing here guesses them. Which events happen together will be the engine's too
//! (by-w8gv.21.74).

use beryl_engine::card::Purpose;

use super::table::Table;
use crate::prompt::{Lies, PlaceName, ShapeName};
use crate::protocol::{
    CardHandling, GameEvent, Kind, PlayerView, Side, Status, TargetRef, Why, ZoneName,
};

/// What is read of one event.
#[derive(Clone, Copy, Debug)]
pub(super) struct Told {
    /// It happens with the event before it that shows anything.
    pub together: bool,
    /// Its cards do not travel: the viewer was already looking at them, or the hand is dealt
    /// before the game.
    pub still: bool,
}
/// What is read of each of `view`'s events, which follow the table `before`. `looked`: the
/// cards the seat's last view had it looking at ([`looked`]).
pub(super) fn tell(before: &Table, view: &PlayerView, looked: &[u16]) -> Vec<Told> {
    let events = &view.events;
    let told = Told {
        together: false,
        still: false,
    };
    let mut told = vec![told; events.len()];
    let pregame = before.turn.number == 0;
    let mut previous = None;
    let mut striking = false;
    for (i, event) in events.iter().enumerate() {
        if quiet(event) {
            // It ends no beat either.
            told[i].together = true;
            continue;
        }
        let combat = matches!(event, GameEvent::Damage { combat: true, .. });
        let life = matches!(
            event,
            GameEvent::LifeGain { .. } | GameEvent::LifeLoss { .. }
        );
        told[i].together = previous.is_some_and(|p: usize| {
            // Combat damage is dealt at once, and a life total changes with the damage.
            (combat && striking)
                || (life && matches!(events[p], GameEvent::Damage { .. }))
                || together(before, &events[p], event)
        });
        striking = combat || (striking && life);
        previous = Some(i);
        told[i].still = still(events, i, looked, pregame);
    }
    told
}

/// Whether event `i`'s cards do not travel.
fn still(events: &[GameEvent], i: usize, looked: &[u16], pregame: bool) -> bool {
    match events[i] {
        GameEvent::ZoneChange {
            slot,
            from: ZoneName::Library,
            to: ZoneName::Graveyard,
            ..
        } => looked.contains(&slot),
        GameEvent::Draw { .. } => pregame,
        // A mulligan's hand goes back to be shuffled.
        GameEvent::ZoneChange {
            from: ZoneName::Hand,
            to: ZoneName::Library,
            ..
        } => {
            pregame
                && matches!(
                    events[i..].iter().find(|e| !to_library(e)),
                    Some(GameEvent::LibraryShuffled { .. })
                )
        }
        _ => false,
    }
}

/// Whether `view` has its seat in the middle of casting a spell, activating an ability or
/// paying as one resolves, with the mana it makes going into what is on top of the stack
/// (`making` in `ManaFlow.tsx`): a cost chooser can follow the payment, before any event
/// announces the spell.
pub(super) fn making(view: &PlayerView) -> bool {
    matches!(
        &view.status,
        Status::Deciding { decision }
            if (decision.kind == Kind::ManaPayment
                && !decision.prompt.is(Purpose::Waterbend))
                || (decision.cancellable && decision.kind != Kind::Priority)
    )
}

fn to_library(event: &GameEvent) -> bool {
    matches!(
        event,
        GameEvent::ZoneChange {
            from: ZoneName::Hand,
            to: ZoneName::Library,
            ..
        }
    )
}

/// Whether `event` shows nothing of its own: it is no end of a beat.
fn quiet(event: &GameEvent) -> bool {
    matches!(
        event,
        GameEvent::Mana { .. }
            | GameEvent::ResolutionPayment { .. }
            | GameEvent::TokensCreated { .. }
            | GameEvent::Lose { .. }
            | GameEvent::Win { .. }
    )
}

/// The order a view's events are scripted in: the order they came in, but that two kinds of
/// event join the like event they belong with, so that the stage plays them as one.
///
/// The combat damage a creature deals back to the creature that strikes it follows that
/// strike at once: combat damage is dealt at one time whatever order its events come in
/// ([CR 510.2](https://yawgatog.com/resources/magic-rules/#R5102): "all combat damage that's
/// been assigned is dealt simultaneously"), and the two blows of one fight are shown as one
/// (`ui/src/stage/tracks.ts`, `meets`).
///
/// A permanent tapped for mana follows the one tapped before it, ahead of the mana that one
/// made: the sources of one payment turn together, and no streak of mana, which leaves a
/// moment after its source turns, holds the next source back.
pub(super) fn order(events: &[GameEvent]) -> Vec<usize> {
    let blow = |i: usize| match events[i] {
        GameEvent::Damage {
            source,
            to: TargetRef::Object { slot },
            combat: true,
            ..
        } => Some((source, slot)),
        _ => None,
    };
    let fighting = |i: usize| matches!(events[i], GameEvent::Damage { combat: true, .. });
    let mana = |i: usize| matches!(events[i], GameEvent::Mana { .. });
    let tap = |i: usize| matches!(events[i], GameEvent::Tap { .. });
    let mut order: Vec<usize> = Vec::with_capacity(events.len());
    for i in 0..events.len() {
        // The strike it answers: the last before it in this combat damage, unanswered yet.
        let answered = blow(i).and_then(|(source, to)| {
            let dealt = order
                .iter()
                .rposition(|&j| !fighting(j) && !quiet(&events[j]));
            let from = dealt.map_or(0, |at| at + 1);
            (from..order.len()).rev().find(|&at| {
                blow(order[at]) == Some((to, source))
                    && order
                        .get(at + 1)
                        .is_none_or(|&next| blow(next) != Some((source, to)))
            })
        });
        // The tap before it, with only mana since.
        let tapped = tap(i)
            .then(|| order.iter().rposition(|&j| !mana(j)))
            .flatten()
            .filter(|&at| tap(order[at]) && at + 1 < order.len());
        match answered.or(tapped) {
            Some(at) => order.insert(at + 1, i),
            None => order.push(i),
        }
    }
    order
}

/// Whether `event` happens with `previous`, the event before it that shows anything.
fn together(before: &Table, previous: &GameEvent, event: &GameEvent) -> bool {
    use GameEvent::{
        Attack, CountersAdded, CountersRemoved, LifeGain, LifeLoss, Reveal, Tap, Untap, ZoneChange,
    };
    use ZoneName::{Battlefield, Exile, Library, Stack};
    match (previous, event) {
        (
            &ZoneChange {
                slot: left,
                from,
                to,
                reason,
                ..
            },
            &ZoneChange {
                slot,
                from: from2,
                to: to2,
                reason: reason2,
                ..
            },
        ) => {
            let leaves = from == Battlefield && to != Battlefield;
            // One move of several cards; permanents leaving for one reason (a sweeper's
            // creatures) whatever their owners' piles.
            (from == from2 && reason == reason2 && (to == to2 || (leaves && to2 != Battlefield)))
                // Cards collected into a library.
                || (to == Library && to2 == Library && from != Library && from2 != Library)
                // A counterspell follows the spell it countered (`counterDeparture`).
                || (from == Stack && reason == Why::Counter && from2 == Stack && reason2 == Why::Resolve)
                // What a permanent held in exile comes back as it leaves
                // (`linkedReturnFlights`).
                || (from2 == Exile
                    && to2 == Battlefield
                    && reason2 == Why::Return
                    && before.objects.get(&slot).and_then(|o| o.exiled_by).is_some_and(|by| {
                        (leaves && by == left)
                            || before.objects.get(&left).and_then(|o| o.exiled_by) == Some(by)
                    }))
        }
        // Permanents tap together, as attackers do when they are declared, and untap
        // together; each player's life changes at once; cards revealed one after another
        // are held up together.
        (Tap { .. } | Attack { .. }, Tap { .. })
        | (Tap { .. }, Attack { .. })
        | (Reveal { .. }, Reveal { .. })
        | (Untap { .. }, Untap { .. })
        | (LifeLoss { .. }, LifeLoss { .. })
        | (LifeGain { .. }, LifeGain { .. }) => true,
        (CountersAdded { counter: a, .. }, CountersAdded { counter: b, .. })
        | (CountersRemoved { counter: a, .. }, CountersRemoved { counter: b, .. }) => a == b,
        _ => false,
    }
}

/// The cards `view` has its seat looking at, or watching the opponent handle: one of them put
/// into a graveyard next is there with no flight (`seenInChoice` and `directToGraveyard` in
/// `beats.ts`; Arena, `iRr-GeSF0lI 21:23.9`).
pub(super) fn looked(view: &PlayerView) -> Vec<u16> {
    let top = |player: Side, count: u8| {
        let library = match player {
            Side::You => &view.players.you.library,
            Side::Opponent => &view.players.opponent.library,
        };
        library
            .iter()
            .rev()
            .take(usize::from(count))
            .copied()
            .collect()
    };
    if let Status::Deciding { decision } = &view.status {
        let prompt = &decision.prompt;
        return match prompt.from {
            // The top of a library, looked at.
            Some(Lies {
                player,
                zone: ZoneName::Library,
            }) if prompt.count > 0 => top(player, prompt.count),
            // A yes or no that sends a card somewhere shows that card.
            _ if prompt.shape == ShapeName::YesNo && prompt.places[0] != PlaceName::None => {
                prompt.about.clone()
            }
            _ => Vec::new(),
        };
    }
    match view.handling {
        Some(CardHandling::Library {
            side,
            count: Some(count),
            ..
        }) => top(side, count),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GameEvent, Side, TargetRef, order};

    fn blow(source: u16, slot: u16) -> GameEvent {
        let to = TargetRef::Object { slot };
        GameEvent::Damage {
            source,
            to,
            amount: 1,
            combat: true,
        }
    }

    /// The blow a blocker deals back follows the strike it answers, whatever was dealt
    /// between them: an attacker's at a player, or the same attacker's at a second blocker.
    #[test]
    fn a_fights_two_blows_are_scripted_together() {
        let player = TargetRef::Player {
            player: Side::Opponent,
        };
        let at_player = GameEvent::Damage {
            source: 2,
            to: player,
            amount: 1,
            combat: true,
        };
        let events = [blow(1, 9), at_player, blow(3, 8), blow(9, 1), blow(8, 3)];
        assert_eq!(order(&events), [0, 3, 1, 2, 4]);
        // One attacker, two blockers: each blocker answers the strike at itself.
        let events = [blow(1, 8), blow(1, 9), blow(8, 1), blow(9, 1)];
        assert_eq!(order(&events), [0, 2, 1, 3]);
    }

    /// The lands of one payment tap before any of their mana leaves; a permanent tapped
    /// after anything else has happened keeps its place.
    #[test]
    fn a_payments_sources_tap_together() {
        let tap = |slot| GameEvent::Tap { slot };
        let mana = |slot| GameEvent::Mana {
            player: Side::You,
            source: Some(slot),
            color: crate::protocol::Mana::White,
            amount: 1,
        };
        let events = [tap(1), mana(1), tap(2), mana(2), tap(3), mana(3)];
        assert_eq!(order(&events), [0, 2, 4, 1, 3, 5]);
        let events = [tap(1), mana(1), blow(7, 8), tap(2), mana(2)];
        assert_eq!(order(&events), [0, 1, 2, 3, 4]);
    }

    /// Damage that is not combat's is no fight, and a later combat's blows answer none of an
    /// earlier one's.
    #[test]
    fn only_one_combat_damages_blows_are_paired() {
        let burn = GameEvent::Damage {
            source: 9,
            to: TargetRef::Object { slot: 1 },
            amount: 1,
            combat: false,
        };
        let events = [blow(1, 9), blow(2, 8), burn.clone(), blow(9, 1)];
        assert_eq!(order(&events), [0, 1, 2, 3]);
    }
}
