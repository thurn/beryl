//! How a script stages a view's events: which beats they are shown in and in what order, and
//! what the viewer was already looking at. Which events happen at once is the engine's word
//! ([`Heard::together`]): events of one group are one beat. What is joined beyond
//! that is presentation, each rule read from the engine's tags with the UI Conventions entry
//! or the Arena timestamp it follows, never from the order the events came in. What the seat
//! was looking at is read from its own last prompt and view.

use beryl_engine::card::Purpose;

use super::hear::{Cause, Heard};
use crate::prompt::{Lies, PlaceName, ShapeName};
use crate::protocol::{
    CardHandling, GameEvent, Kind, PlayerView, Side, Status, TargetRef, Why, ZoneName,
};

/// How one event is staged.
#[derive(Clone, Copy, Debug)]
pub(super) struct Told {
    /// It is shown in the beat of the event before it that shows anything.
    pub together: bool,
    /// Its cards do not travel: the viewer was already looking at them, or the hand is dealt
    /// before the game.
    pub still: bool,
}

/// How each of `view`'s events is staged, with what the engine said of them (`heard`, its
/// causes resized to the events). `looked`: the cards the seat's last view had it looking at
/// ([`looked`]); `pregame`: the view follows a table before the first turn.
pub(super) fn tell(
    view: &PlayerView,
    heard: &Heard,
    causes: &[Cause],
    looked: &[u16],
    pregame: bool,
) -> Vec<Told> {
    let events = &view.events;
    // The engine's group of each event ([`Heard::together`]): its word, never guessed.
    let mut group = 0_usize;
    let groups: Vec<usize> = (0..events.len())
        .map(|i| {
            if i > 0 && !heard.together.get(i).copied().unwrap_or(false) {
                group += 1;
            }
            group
        })
        .collect();
    let mut told = Vec::with_capacity(events.len());
    let mut previous: Option<usize> = None;
    for (i, event) in events.iter().enumerate() {
        let together = quiet(event)
            || previous.is_some_and(|p| {
                groups[p] == groups[i] || staged(&events[p], causes[p], event, causes[i])
            });
        if !quiet(event) {
            previous = Some(i);
        }
        let still = still(events, i, looked, pregame);
        told.push(Told { together, still });
    }
    told
}

/// Whether `event`, of the cause `by`, is shown in the beat of `previous`, of the cause
/// `was`, though the rules did not do the two at once: what the stage shows as one.
fn staged(previous: &GameEvent, was: Cause, event: &GameEvent, by: Cause) -> bool {
    use GameEvent::{Reveal, Tap, ZoneChange};
    let one = was.entry().is_some() && was.entry() == by.entry();
    match (previous, event) {
        // The sources of one payment turn together, each paid for apart where the player
        // picks them: "Mana leaves as its source begins to turn, several into one spell
        // together" (UI Conventions, "streak").
        (Tap { .. }, Tap { .. }) => one && matches!(by, Cause::Player { .. }),
        // One effect's revealed cards lie in the pile together (UI Conventions, "Cards
        // revealed"; Arena, `I3Te0m1bBFw 22:08.8`).
        (Reveal { .. }, Reveal { .. }) => one && was == by,
        // A countered spell leaves before the spell that countered it, which goes in the
        // same beat (UI Conventions, "A countered spell", by-kxyq; Arena,
        // `I3Te0m1bBFw 39:50.0`): the countered spell's move is the counterspell's effect,
        // and the counterspell's is the end of its own resolution.
        (
            ZoneChange {
                from: ZoneName::Stack,
                reason: Why::Counter,
                ..
            },
            ZoneChange {
                from: ZoneName::Stack,
                reason: Why::Resolve,
                ..
            },
        ) => one,
        _ => false,
    }
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
/// event move up within their beat (`told`, [`tell`]) to the like event they belong with, so
/// that the stage plays them as one.
///
/// The combat damage a creature deals back to the creature that strikes it follows that
/// strike: combat damage is dealt at one time whatever order its events come in
/// ([CR 510.2](https://yawgatog.com/resources/magic-rules/#R5102): "all combat damage that's
/// been assigned is dealt simultaneously"), its events one group, and the two blows of one
/// fight are shown as one (UI Conventions, "strike": "Two creatures that strike each other
/// meet half way, at one time"; `ui/src/stage/tracks.ts`, `meets`).
///
/// A permanent tapped for mana follows the one tapped before it in its beat, ahead of the
/// mana that one made: the sources of one payment turn together, and no streak of mana,
/// which leaves a moment after its source turns, holds the next source back (UI Conventions,
/// "streak": "Mana leaves as its source begins to turn").
pub(super) fn order(events: &[GameEvent], told: &[Told]) -> Vec<usize> {
    let blow = |i: usize| match events[i] {
        GameEvent::Damage {
            source,
            to: TargetRef::Object { slot },
            combat: true,
            ..
        } => Some((source, slot)),
        _ => None,
    };
    let mana = |i: usize| matches!(events[i], GameEvent::Mana { .. });
    let tap = |i: usize| matches!(events[i], GameEvent::Tap { .. });
    let mut order: Vec<usize> = Vec::with_capacity(events.len());
    for i in 0..events.len() {
        // Where the beat it joins begins: only a beat's own events are moved past.
        let Some(beat) = order
            .iter()
            .rposition(|&j| !told[j].together)
            .filter(|_| told[i].together)
        else {
            order.push(i);
            continue;
        };
        // The strike it answers, in its beat and unanswered yet.
        let answered = blow(i).and_then(|(source, to)| {
            (beat..order.len()).rev().find(|&at| {
                blow(order[at]) == Some((to, source))
                    && order
                        .get(at + 1)
                        .is_none_or(|&next| blow(next) != Some((source, to)))
            })
        });
        // The tap before it in its beat, with only mana since.
        let tapped = tap(i)
            .then(|| order.iter().rposition(|&j| !mana(j)))
            .flatten()
            .filter(|&at| at >= beat && tap(order[at]) && at + 1 < order.len());
        match answered.or(tapped) {
            Some(at) => order.insert(at + 1, i),
            None => order.push(i),
        }
    }
    order
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
    use super::{GameEvent, Side, TargetRef, Told, order};

    /// Beats of the given lengths, in order.
    fn beats(lengths: &[usize]) -> Vec<Told> {
        let told = |together| Told {
            together,
            still: false,
        };
        let beat = |n: usize| (0..n).map(move |at| told(at > 0));
        lengths.iter().flat_map(|&n| beat(n)).collect()
    }

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
        assert_eq!(order(&events, &beats(&[5])), [0, 3, 1, 2, 4]);
        // One attacker, two blockers: each blocker answers the strike at itself.
        let events = [blow(1, 8), blow(1, 9), blow(8, 1), blow(9, 1)];
        assert_eq!(order(&events, &beats(&[4])), [0, 2, 1, 3]);
    }

    /// The lands of one payment tap before any of their mana leaves; a permanent tapped in a
    /// beat of its own keeps its place.
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
        assert_eq!(order(&events, &beats(&[6])), [0, 2, 4, 1, 3, 5]);
        let events = [tap(1), mana(1), tap(2), mana(2)];
        assert_eq!(order(&events, &beats(&[2, 2])), [0, 1, 2, 3]);
    }

    /// Only a beat's blows are paired: a later combat's answer none of an earlier one's.
    #[test]
    fn only_one_combat_damages_blows_are_paired() {
        let burn = GameEvent::Damage {
            source: 9,
            to: TargetRef::Object { slot: 1 },
            amount: 1,
            combat: false,
        };
        let events = [blow(1, 9), blow(2, 8), burn.clone(), blow(9, 1)];
        assert_eq!(order(&events, &beats(&[2, 1, 1])), [0, 1, 2, 3]);
    }
}
