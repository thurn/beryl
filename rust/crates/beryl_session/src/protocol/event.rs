//! The game events a view carries, built from the engine's events.

use beryl_engine::event::{Event, Recipient};
use beryl_engine::state::StackEntry;
use beryl_engine::{GameTable, Layout, Observation, SlotId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{
    Counter, Mana, Side, StackItem, Step, TargetRef, Why, ZoneName, side, slots, stack_item,
    stack_item_cast,
};
use crate::script::{Heard, Move, Said, Subject};

/// One thing that happened (architecture §8.1), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameEvent {
    /// The object now in `slot` arrived from another zone.
    ZoneChange {
        slot: u16,
        from: ZoneName,
        to: ZoneName,
        reason: Why,
        /// Whether a spell went to the top or the bottom of its owner's library.
        #[serde(rename = "libraryTop", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        library_top: Option<bool>,
    },
    Damage {
        source: u16,
        to: TargetRef,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Side,
        cards: u8,
        /// The public slots drawn, top first. Older recordings carry only the count.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        slots: Option<Vec<u16>>,
    },
    Scry {
        player: Side,
        top: u8,
        bottom: u8,
    },
    LibraryShuffled {
        player: Side,
    },
    /// The card in `slot` was revealed to every player
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time."
    Reveal {
        slot: u16,
    },
    LifeGain {
        player: Side,
        amount: u8,
    },
    LifeLoss {
        player: Side,
        amount: u8,
    },
    CountersAdded {
        slot: u16,
        counter: Counter,
        amount: u8,
    },
    CountersRemoved {
        slot: u16,
        counter: Counter,
        amount: u8,
    },
    Tap {
        slot: u16,
    },
    Untap {
        slot: u16,
    },
    /// Mana added to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." The UI
    /// shows it flow from its source to the spell or ability it pays for.
    Mana {
        player: Side,
        /// The mana ability's permanent, or the spell or ability that added it, in the slot
        /// it had then: a source sacrificed for mana has left it by the time the view shows.
        source: Option<u16>,
        color: Mana,
        amount: u8,
    },
    /// Mana paid into the named resolving spell or ability.
    ResolutionPayment {
        player: Side,
        item: StackItem,
    },
    /// The top ability finished resolving and left the stack.
    AbilityFinished,
    TokensCreated {
        controller: Side,
        tokens: Vec<u16>,
    },
    Cast {
        slot: u16,
        controller: Side,
        /// The spell as it went on the stack: an opponent's spell that resolves unanswered
        /// is gone from the stack by the next view, which shows its cast and resolution
        /// together.
        item: StackItem,
    },
    /// An ability was activated, or a triggered one with targets put on the stack, as the
    /// stack then showed it.
    Ability {
        item: StackItem,
        /// A loyalty ability's cost: the loyalty counters it added or removed, which the
        /// activation says rather than the counters
        /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)): "The cost to activate
        /// a loyalty ability of a permanent is to put on or remove from that permanent a certain
        /// number of loyalty counters, as shown by the loyalty symbol in the ability's cost."
        loyalty: Option<i8>,
    },
    Attack {
        player: Side,
        attackers: Vec<u16>,
    },
    StepBegins {
        step: Step,
        active: Side,
    },
    GainControl {
        slot: u16,
        player: Side,
    },
    CoinFlip {
        player: Side,
        won: bool,
        /// The permanent whose ability flipped it.
        source: u16,
    },
    Lose {
        player: Side,
    },
    Win {
        player: Side,
    },
}

/// What damage was dealt to, as the viewer names it.
pub(super) fn recipient<L: Layout>(obs: Observation<'_, L>, to: Recipient<L>) -> TargetRef {
    match to {
        Recipient::Player(player) => TargetRef::Player {
            player: side(obs, player),
        },
        Recipient::Permanent(o) => TargetRef::Object { slot: o.slot.num() },
    }
}

/// An [`Event::Ability`] as the viewer of `obs` is told it: as the stack showed it.
fn ability_event<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let Event::Ability {
        source,
        controller,
        card,
        face,
        ability,
        targets,
        x,
    } = e.clone()
    else {
        unreachable!("an ability's event");
    };
    let entry = StackEntry {
        object: source,
        controller,
        ability: Some(ability),
        card,
        face,
        targets,
        x,
        ..StackEntry::default()
    };
    GameEvent::Ability {
        item: stack_item(t, obs, &entry),
        loyalty: t
            .def(card)
            .face(face)
            .ability(ability)
            .cost()
            .loyalty_change(),
    }
}

/// The spell an [`Event::Cast`] put on the stack, as the stack showed it then. Its card is
/// public on the stack ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "a
/// player first moves that card ... from where it is to the stack"), and `obs` names it while
/// it stays public.
fn cast_item<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> StackItem {
    let Event::Cast {
        spell,
        controller,
        targets,
        x,
        costs,
        flashback,
    } = e.clone()
    else {
        unreachable!("a cast's event");
    };
    // A spell gone back to a hidden zone since (bounced to its owner's hand) keeps its slot,
    // its targets and X; the costs its card was cast with are its card's to say.
    let entry = match obs.card(spell.slot) {
        Some(card) => StackEntry {
            card,
            face: obs.object(spell.slot).face(),
            costs,
            ..StackEntry::default()
        },
        None => StackEntry::default(),
    };
    let entry = StackEntry {
        object: Some(spell),
        controller,
        targets,
        x,
        ..entry
    };
    stack_item_cast(t, obs, &entry, flashback && obs.card(spell.slot).is_some())
}

/// An [`Event::CountersAdded`] or [`Event::CountersRemoved`] as the viewer is told it.
fn counters_event<L: Layout>(e: &Event<L>) -> GameEvent {
    match *e {
        Event::CountersAdded {
            object,
            kind,
            amount,
        } => GameEvent::CountersAdded {
            slot: object.slot.num(),
            counter: kind.into(),
            amount,
        },
        Event::CountersRemoved {
            object,
            kind,
            amount,
        } => GameEvent::CountersRemoved {
            slot: object.slot.num(),
            counter: kind.into(),
            amount,
        },
        _ => unreachable!("a counters event"),
    }
}

/// An [`Event::Mana`] as the viewer of `obs` is told it: who made it, from what, all public
/// ([CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): a mana ability
/// "resolves immediately after it is activated", in view of both players).
fn mana_event<L: Layout>(obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let &Event::Mana {
        player,
        source,
        color,
        amount,
    } = e
    else {
        unreachable!("a mana event")
    };
    GameEvent::Mana {
        player: side(obs, player),
        source: source.map(|o| o.slot.num()),
        color: color.into(),
        amount,
    }
}

fn library_event<L: Layout>(obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let side = |p| side(obs, p);
    match *e {
        Event::Draw { player, cards } => GameEvent::Draw {
            player: side(player),
            cards,
            slots: None,
        },
        Event::Scry {
            player,
            top,
            bottom,
        } => GameEvent::Scry {
            player: side(player),
            top,
            bottom,
        },
        Event::LibraryShuffled { player } => GameEvent::LibraryShuffled {
            player: side(player),
        },
        _ => unreachable!("not a library event"),
    }
}

/// Combine the individual public handles with the draw count the rules listen to.
pub(super) fn game_events<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    events: &[Event<L>],
) -> Vec<GameEvent> {
    let mut drawn = Vec::new();
    events
        .iter()
        .filter_map(|e| {
            if let Event::DrawCard { slot } = e {
                drawn.push(slot.num());
                return None;
            }
            // Said for whoever follows the stack through the events, which a seat's screen
            // does ([`heard`]): a view shows the stack itself, and the blocks declared.
            if let Event::StackArrival { .. } | Event::StackDeparture { .. } | Event::Block { .. } =
                e
            {
                return None;
            }
            let mut event = game_event(t, obs, e);
            if let GameEvent::Draw { slots, .. } = &mut event {
                *slots = (!drawn.is_empty()).then(|| std::mem::take(&mut drawn));
            }
            Some(event)
        })
        .collect()
}

/// What the engine said of `events` beyond what [`game_events`] tells the viewer of `obs`
/// (plan K23), for their screen to build its script from: what each of the view's events came
/// from, as `said` has it of each of `events`, and the abilities that arrived on the stack
/// and left it between them. `stack` names the entries of the view's stack
/// ([`Heard::stack`]).
pub fn heard<L: Layout>(
    t: &GameTable,
    obs: Observation<'_, L>,
    events: &[Event<L>],
    said: &[Said],
    stack: Vec<Subject>,
) -> Heard {
    let mut heard = Heard {
        stack,
        ..Heard::default()
    };
    for (event, said) in events.iter().zip(said) {
        let cause = said.cause.of(|player| side(obs, player));
        let before = heard.causes.len();
        match (event, said.moved) {
            (Event::StackArrival { entry }, Some(Subject::Ability { id })) => {
                let item = stack_item(t, obs, entry);
                heard
                    .moves
                    .push((before, Move::Arrival { id, item, cause }));
            }
            (Event::StackDeparture { .. }, Some(Subject::Ability { id })) => {
                heard.moves.push((before, Move::Departure { id, cause }));
            }
            // A view's draw says its cards, a spell arrives and leaves as its card moves, and
            // no view is told a block.
            (
                Event::DrawCard { .. }
                | Event::StackArrival { .. }
                | Event::StackDeparture { .. }
                | Event::Block { .. },
                _,
            ) => {}
            _ => heard.causes.push(cause),
        }
    }
    heard
}

/// An engine event as the viewer of `obs` is told it. Engine events name objects by slot and
/// carry no identity: anything an event says about a card must come from `obs`, which the
/// information independence test redeals. The table names only an ability's card, public as
/// the stack shows it.
fn game_event<L: Layout>(t: &GameTable, obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let side = |p| side(obs, p);
    match *e {
        Event::ZoneChange {
            object,
            from,
            to,
            reason,
            library_top,
        } => GameEvent::ZoneChange {
            slot: object.slot.num(),
            from: from.into(),
            to: to.into(),
            reason: reason.into(),
            library_top,
        },
        Event::Damage {
            source,
            to,
            amount,
            combat,
        } => GameEvent::Damage {
            source: source.slot.num(),
            to: recipient(obs, to),
            amount,
            combat,
        },
        Event::Draw { .. } | Event::Scry { .. } | Event::LibraryShuffled { .. } => {
            library_event(obs, e)
        }
        Event::DrawCard { .. } => unreachable!("draw slots are grouped with their draw"),
        Event::Reveal { slot } => GameEvent::Reveal { slot: slot.num() },
        Event::LifeGain { player, amount } => GameEvent::LifeGain {
            player: side(player),
            amount,
        },
        Event::LifeLoss { player, amount } => GameEvent::LifeLoss {
            player: side(player),
            amount,
        },
        Event::CountersAdded { .. } | Event::CountersRemoved { .. } => counters_event(e),
        Event::Tap { object } => GameEvent::Tap {
            slot: object.slot.num(),
        },
        Event::Untap { object } => GameEvent::Untap {
            slot: object.slot.num(),
        },
        Event::Mana { .. } => mana_event(obs, e),
        Event::ResolutionPayment { player, ref item } => GameEvent::ResolutionPayment {
            player: side(player),
            item: stack_item(t, obs, item),
        },
        Event::AbilityFinished => GameEvent::AbilityFinished,
        Event::StackArrival { .. } | Event::StackDeparture { .. } | Event::Block { .. } => {
            unreachable!("the stack's arrivals and departures and the blocks are in no view")
        }
        Event::TokensCreated { controller, tokens } => GameEvent::TokensCreated {
            controller: side(controller),
            tokens: slots(obs.in_candidate_order(tokens)),
        },
        Event::Cast {
            spell, controller, ..
        } => GameEvent::Cast {
            slot: spell.slot.num(),
            controller: side(controller),
            item: cast_item(t, obs, e),
        },
        Event::Ability { .. } => ability_event(t, obs, e),
        Event::Attack { player, attackers } => GameEvent::Attack {
            player: side(player),
            attackers: slots(obs.in_candidate_order(attackers)),
        },
        Event::StepBegins { step, active } => GameEvent::StepBegins {
            step: step.into(),
            active: side(active),
        },
        Event::GainControl { object, player } => GameEvent::GainControl {
            slot: object.slot.num(),
            player: side(player),
        },
        Event::CoinFlip {
            player,
            won,
            source,
        } => GameEvent::CoinFlip {
            player: side(player),
            won,
            source: source.slot.num(),
        },
        Event::Lose { player, .. } => GameEvent::Lose {
            player: side(player),
        },
        Event::Win { player } => GameEvent::Win {
            player: side(player),
        },
    }
}
