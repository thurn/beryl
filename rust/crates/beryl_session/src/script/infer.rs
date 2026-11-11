//! Every guess a script is built on, in one place: what an event came from, which events
//! happen together, which entry of the stack an event is about, and what the viewer was
//! already looking at. The engine knew all of it and today's flat events do not say, so this
//! reads it back out of their order, as `ui/src/game/beats.ts` did. It is throwaway: the
//! interpreter will say these things itself (plan K23, by-w8gv.21.5), and the builder, which
//! takes them as given, stays as it is.

use beryl_engine::card::Purpose;

use super::table::{Entry, Table};
use crate::prompt::{Lies, PlaceName, ShapeName};
use crate::protocol::{
    CardHandling, Counter, GameEvent, Kind, PlayerView, Side, StackItem, Status, Why, ZoneName,
};

/// What a step comes from, before the builder names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// The rules of the game: a turn-based or state-based action.
    Rules,
    /// A player's own action: a land played, a spell cast, a cost paid.
    Player(Side),
    /// A card: a spell resolving, the source of damage, the permanent that flipped a coin.
    Object(u16),
    /// The ability on top of the stack, which is resolving.
    Ability,
    /// Whatever is on top of the stack, in the middle of its resolution.
    Top,
}

/// What is guessed of one event.
#[derive(Clone, Copy, Debug)]
pub(super) struct Told {
    pub cause: Origin,
    /// It happens with the event before it that shows anything.
    pub together: bool,
    pub show: Show,
    /// What a `Mana` event's mana pays for.
    pub pays: Pays,
    /// What it casts, activates or pays into takes the mana its player's pool lost since the
    /// view before: the first of a player's in a script, with no step ending in it to empty
    /// the pool ([CR 500.5](https://yawgatog.com/resources/magic-rules/#R5005): "As a step
    /// or phase ends ... any unspent mana left in a player's mana pool empties").
    pub drains: bool,
}

/// What mana is spent on, as `spentOn` in `ManaFlow.tsx` read it out of the events' order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pays {
    /// Nothing in this script: it is left in its player's pool.
    Pool,
    /// The spell cast from the card in this slot.
    Spell(u16),
    /// The ability the event at this index announces, or the resolving spell or ability the
    /// payment at this index names.
    Event(usize),
    /// The spell or ability in the making on top of the stack.
    Top,
}

/// How an event shows, beyond what its kind says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Show {
    Plain,
    /// The spell or ability it announces is left to be read: the opponent's, gone by the time
    /// the view shows.
    Dwell,
    /// Its cards do not travel: the viewer was already looking at them, or the hand is dealt
    /// before the game.
    Still,
    /// It shows once the ability it pays for is on the stack: a loyalty cost, which the
    /// events put before the ability (Arena, `5UHzpVvUkfI 19:59.25`).
    Late,
}

/// An event of which nothing is guessed.
pub(super) const TOLD: Told = Told {
    cause: Origin::Rules,
    together: false,
    show: Show::Plain,
    pays: Pays::Pool,
    drains: false,
};

/// What each of `view`'s events is guessed to be, between the tables `before` and `after`.
/// `resolving`: the view is in the middle of a resolution. `looked`: the cards the seat's
/// last view had it looking at ([`looked`]).
pub(super) fn tell(
    before: &Table,
    after: &Table,
    view: &PlayerView,
    resolving: bool,
    looked: &[u16],
) -> Vec<Told> {
    let events = &view.events;
    let mut told = vec![TOLD; events.len()];
    causes(events, before, after, resolving, &mut told);
    payments(events, after, making(view), &mut told);
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
        told[i].show = show(events, i, after, looked, pregame);
    }
    told
}

/// How event `i` shows.
fn show(events: &[GameEvent], i: usize, after: &Table, looked: &[u16], pregame: bool) -> Show {
    let still = match events[i] {
        GameEvent::Cast {
            controller: Side::Opponent,
            ref item,
            ..
        }
        | GameEvent::Ability {
            item:
                ref item @ StackItem::Ability {
                    controller: Side::Opponent,
                    ..
                },
            ..
        } => {
            let stays = after.stack.iter().any(|e| same(&e.item, item));
            return if stays { Show::Plain } else { Show::Dwell };
        }
        GameEvent::CountersAdded { slot, counter, .. }
        | GameEvent::CountersRemoved { slot, counter, .. } => {
            let pays = counter == Counter::Loyalty
                && matches!(
                    events[i..].iter().find(|e| ends_costs(e)),
                    Some(GameEvent::Ability {
                        item: StackItem::Ability { source, .. },
                        loyalty: Some(_),
                    }) if *source == Some(slot)
                );
            return if pays { Show::Late } else { Show::Plain };
        }
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
    };
    if still { Show::Still } else { Show::Plain }
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

/// Who pays as `event` says: the player casting, activating, or paying into a resolution.
fn payer(event: &GameEvent) -> Option<Side> {
    match *event {
        GameEvent::Cast { controller, .. }
        | GameEvent::Ability {
            item:
                StackItem::Ability {
                    controller,
                    triggered: false,
                    ..
                },
            ..
        } => Some(controller),
        GameEvent::ResolutionPayment { player, .. } => Some(player),
        _ => None,
    }
}

/// What each `Mana` event's mana pays for, and which events take what a pool lost. A cost is
/// paid as the last step of casting a spell or activating an ability, before either is
/// announced here ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The
/// player pays the total cost in any order"), so mana is spent on its player's next cast,
/// activation or payment in the script; with none, on the spell being cast as the view
/// shows; else it is left in the pool.
fn payments(events: &[GameEvent], after: &Table, making: bool, told: &mut [Told]) {
    let empties = events
        .iter()
        .any(|e| matches!(e, GameEvent::StepBegins { .. }));
    let mut drained = Vec::new();
    for (i, event) in events.iter().enumerate() {
        if let GameEvent::Mana { player, .. } = *event {
            told[i].pays = paid_for(events, i, player, after, making);
        } else if let Some(player) = payer(event)
            && !empties
            && !drained.contains(&player)
        {
            told[i].drains = true;
            drained.push(player);
        }
    }
}

fn paid_for(events: &[GameEvent], i: usize, player: Side, after: &Table, making: bool) -> Pays {
    let next = (i + 1..events.len()).find(|&j| payer(&events[j]) == Some(player));
    if let Some(j) = next {
        return match events[j] {
            GameEvent::Cast { slot, .. } => Pays::Spell(slot),
            _ => Pays::Event(j),
        };
    }
    if making && player == Side::You {
        return Pays::Top;
    }
    // A spell this script puts on the stack, whose cast is not yet announced.
    let cast = |slot: u16| {
        let puts = |e: &GameEvent| matches!(*e, GameEvent::ZoneChange { slot: s, to: ZoneName::Stack, reason: Why::Cast, .. } if s == slot);
        events.iter().any(puts)
            && after
                .objects
                .get(&slot)
                .is_some_and(|o| o.controller == player)
    };
    after
        .stack
        .iter()
        .find_map(|entry| match entry.item {
            StackItem::Spell { slot, .. } if cast(slot) => Some(Pays::Spell(slot)),
            _ => None,
        })
        .unwrap_or(Pays::Pool)
}

/// Whether `event` ends the costs before it: what they paid for is announced, or something
/// else began.
fn ends_costs(event: &GameEvent) -> bool {
    matches!(
        event,
        GameEvent::Ability { .. }
            | GameEvent::Cast { .. }
            | GameEvent::AbilityFinished
            | GameEvent::StepBegins { .. }
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

/// What each event came from. The events run in segments, each ended by an event that says
/// what the segment was: a spell cast or an ability activated (the costs before it are its
/// controller's doing), a spell or ability finishing (the events before it are its effects), a
/// step beginning or an attack (what came before was loose: the rules, or a player's own
/// action). The last segment has no end yet: effects in the middle of a resolution, else
/// loose.
fn causes(events: &[GameEvent], before: &Table, after: &Table, resolving: bool, told: &mut [Told]) {
    let loose = |i: usize| loose(events, i, before, after);
    let mut start = 0;
    for (i, event) in events.iter().enumerate() {
        let (inside, own) = match *event {
            GameEvent::Cast {
                slot, controller, ..
            } => {
                // The cast begins as its card goes on the stack: what came before was loose.
                let puts = |e: &GameEvent| matches!(*e, GameEvent::ZoneChange { slot: s, to: ZoneName::Stack, .. } if s == slot);
                let begins = (start..i).find(|&j| puts(&events[j]));
                assign(told, start, begins.unwrap_or(start), &loose);
                start = begins.unwrap_or(start);
                let player = Origin::Player(controller);
                (Some(player), player)
            }
            GameEvent::Ability {
                item:
                    StackItem::Ability {
                        controller,
                        triggered: false,
                        ..
                    },
                ..
            } => {
                let player = Origin::Player(controller);
                (Some(player), player)
            }
            GameEvent::AbilityFinished => (Some(Origin::Ability), Origin::Rules),
            GameEvent::ZoneChange {
                slot,
                from: ZoneName::Stack,
                reason: Why::Resolve,
                ..
            } => (Some(Origin::Object(slot)), Origin::Rules),
            GameEvent::Attack { player, .. } => (None, Origin::Player(player)),
            GameEvent::Ability { .. } | GameEvent::StepBegins { .. } => (None, Origin::Rules),
            _ => continue,
        };
        assign(told, start, i, &|j| inside.unwrap_or_else(|| loose(j)));
        told[i].cause = own;
        start = i + 1;
    }
    let rest = |j| if resolving { Origin::Top } else { loose(j) };
    assign(told, start, events.len(), &rest);
    for (i, event) in events.iter().enumerate() {
        if let GameEvent::CoinFlip { source, .. } = *event {
            told[i].cause = Origin::Object(source);
        }
    }
}

/// Gives the events from `start` up to `end` the cause `cause` finds for each.
fn assign(told: &mut [Told], start: usize, end: usize, cause: &dyn Fn(usize) -> Origin) {
    for (j, told) in told.iter_mut().enumerate().take(end).skip(start) {
        told.cause = cause(j);
    }
}

/// What event `i` came from when nothing was resolving and nothing being cast.
fn loose(events: &[GameEvent], i: usize, before: &Table, after: &Table) -> Origin {
    let controller = |slot: u16| {
        after
            .objects
            .get(&slot)
            .or_else(|| before.objects.get(&slot))
            .map_or(Origin::Rules, |o| Origin::Player(o.controller))
    };
    match events[i] {
        GameEvent::ZoneChange {
            slot,
            reason: Why::Play | Why::Cast,
            ..
        } => controller(slot),
        // A token made as a permanent enters comes from that permanent, if it is the only
        // one (`tokenOrigins` in `Board.tsx`).
        GameEvent::ZoneChange {
            from: ZoneName::Nowhere,
            ..
        } => {
            let mut arrivals = events.iter().filter_map(|e| match *e {
                GameEvent::ZoneChange {
                    slot,
                    from,
                    to: ZoneName::Battlefield,
                    ..
                } if from != ZoneName::Nowhere => Some(slot),
                _ => None,
            });
            match (arrivals.next(), arrivals.next()) {
                (Some(slot), None) => Origin::Object(slot),
                _ => Origin::Rules,
            }
        }
        // A permanent tapped for mana is its controller's doing; one tapped to attack, or
        // untapped as the turn begins, the rules'.
        GameEvent::Tap { slot } => events[i + 1..]
            .iter()
            .take_while(|e| matches!(e, GameEvent::Tap { .. } | GameEvent::Mana { .. }))
            .find_map(|e| match *e {
                GameEvent::Mana {
                    player,
                    source: Some(source),
                    ..
                } if source == slot => Some(Origin::Player(player)),
                _ => None,
            })
            .unwrap_or(Origin::Rules),
        GameEvent::Damage {
            source,
            combat: false,
            ..
        } => Origin::Object(source),
        _ => Origin::Rules,
    }
}

/// Whether `event` happens with `previous`, the event before it that shows anything.
fn together(before: &Table, previous: &GameEvent, event: &GameEvent) -> bool {
    use GameEvent::{
        Attack, CountersAdded, CountersRemoved, LifeGain, LifeLoss, Tap, Untap, ZoneChange,
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
        // together; each player's life changes at once.
        (Tap { .. } | Attack { .. }, Tap { .. })
        | (Tap { .. }, Attack { .. })
        | (Untap { .. }, Untap { .. })
        | (LifeLoss { .. }, LifeLoss { .. })
        | (LifeGain { .. }, LifeGain { .. }) => true,
        (CountersAdded { counter: a, .. }, CountersAdded { counter: b, .. })
        | (CountersRemoved { counter: a, .. }, CountersRemoved { counter: b, .. }) => a == b,
        _ => false,
    }
}

/// Whether `a` and `b` are one spell or ability at two moments: a view's stack does not name
/// its items, and targets and X may change while one is there (`stackKey` in `beats.ts`).
pub(super) fn same(a: &StackItem, b: &StackItem) -> bool {
    match (a, b) {
        (StackItem::Spell { slot: a, .. }, StackItem::Spell { slot: b, .. }) => a == b,
        (
            StackItem::Ability {
                source,
                ability,
                granted,
                name,
                triggered,
                controller,
                ..
            },
            StackItem::Ability {
                source: source2,
                ability: ability2,
                granted: granted2,
                name: name2,
                triggered: triggered2,
                controller: controller2,
                ..
            },
        ) => {
            (source, ability, granted, name, triggered, controller)
                == (source2, ability2, granted2, name2, triggered2, controller2)
        }
        _ => false,
    }
}

/// For each entry of `stack`, bottom first, the entry of `after` that is the same one still
/// there, if any: the entries of both in order.
pub(super) fn pair(stack: &[Entry], after: &[Entry]) -> Vec<Option<usize>> {
    let mut next = 0;
    stack
        .iter()
        .map(|entry| {
            let found = (next..after.len()).find(|&j| same(&entry.item, &after[j].item))?;
            next = found + 1;
            Some(found)
        })
        .collect()
}

/// Whether the triggered ability `item` comes from the permanent in `slot`: it lifts out of a
/// permanent that is leaving before that goes (Arena, `I3Te0m1bBFw 26:56.75`).
pub(super) fn lifts_from(item: &StackItem, slot: u16) -> bool {
    matches!(
        item,
        StackItem::Ability {
            source: Some(source),
            triggered: true,
            ..
        } if *source == slot
    )
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
