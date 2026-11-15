//! The game events a view carries, built from the engine's events.

use beryl_engine::event::{Event, Recipient};
use beryl_engine::state::{self, StackEntry};
use beryl_engine::{GameTable, Layout, Observation, Place, SlotId};
use super::{
    GameEvent, ObjectId, ObjectView, StackItem, TargetRef, ZoneName, side, slots, stack_item,
    stack_item_cast,
};
use crate::script::{Copied, Heard, Move, Said, Subject};

/// What damage was dealt to, as the viewer names it.
pub(super) fn recipient<L: Layout>(obs: Observation<'_, L>, to: Recipient<L>) -> TargetRef {
    match to {
        Recipient::Player(player) => TargetRef::Player {
            player: side(obs, player),
        },
        Recipient::Permanent(o) => TargetRef::Object {
            slot: ObjectId(o.slot.num()),
        },
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
        item: state::StackItem::Activated {
            of: state::AbilityRef {
                source,
                card,
                face,
                ability,
                controller,
            },
        },
        targets,
        x,
        words: state::AS_PRINTED,
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
    let (card, face, costs) = match obs.card(spell.slot) {
        Some(card) => (card, obs.object(spell.slot).face(), costs),
        None => Default::default(),
    };
    let entry = StackEntry {
        item: state::StackItem::Spell {
            object: spell,
            controller,
            card,
            face,
            costs,
        },
        targets,
        x,
        words: state::AS_PRINTED,
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
            slot: ObjectId(object.slot.num()),
            counter: kind.into(),
            amount,
        },
        Event::CountersRemoved {
            object,
            kind,
            amount,
        } => GameEvent::CountersRemoved {
            slot: ObjectId(object.slot.num()),
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
        ..
    } = e
    else {
        unreachable!("a mana event")
    };
    GameEvent::Mana {
        player: side(obs, player),
        source: source.map(|o| ObjectId(o.slot.num())),
        color: color.into(),
        amount,
    }
}

fn block_event<L: Layout>(obs: Observation<'_, L>, e: &Event<L>) -> GameEvent {
    let Event::Block {
        player,
        blocker,
        attacker,
    } = *e
    else {
        unreachable!("a block")
    };
    GameEvent::Block {
        player: side(obs, player),
        blocker: ObjectId(blocker.slot.num()),
        attacker: ObjectId(attacker.slot.num()),
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
        Event::TopOrBottom { player, top } => GameEvent::TopOrBottom {
            player: side(player),
            top,
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
                drawn.push(ObjectId(slot.num()));
                return None;
            }
            // Said for whoever follows the stack through the events, which a seat's screen
            // does ([`heard`]): a view shows the stack itself.
            if let Event::StackArrival { .. } | Event::StackDeparture { .. } = e {
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
///, for their screen to build its script from: what each of the view's events came
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
    // The engine's group of each event, numbered as [`beryl_engine::event::Tag::groups`]
    // does, and that of the latest event the view tells.
    let mut group = 0_u32;
    let mut told = None;
    for (i, (event, said)) in events.iter().zip(said).enumerate() {
        if i > 0 && !said.together {
            group += 1;
        }
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
            (Event::StackArrival { entry }, Some(Subject::Object { .. }))
                if entry
                    .spell()
                    .is_some_and(|spell| matches!(spell.slot.place(), Place::Pool(_))) =>
            {
                heard.copies.push((before, copied(t, obs, entry)));
            }
            // A view's draw says its cards, and a spell arrives and leaves as its card moves.
            (
                Event::DrawCard { .. } | Event::StackArrival { .. } | Event::StackDeparture { .. },
                _,
            ) => {}
            _ => {
                heard.causes.push(cause);
                heard.together.push(told == Some(group));
                told = Some(group);
            }
        }
    }
    heard
}

/// The copy of a spell that `entry` puts on the stack, as every player sees it there: a copy
/// in a slot of the shared pool, owned and controlled by the player who put it on the stack,
/// with the name of the card it copies
/// ([CR 707.10](https://yawgatog.com/resources/magic-rules/#R70710)): "A copy of a spell is
/// owned by the player under whose control it was put on the stack."
fn copied<L: Layout>(t: &GameTable, obs: Observation<'_, L>, entry: &StackEntry<L>) -> Copied {
    let item = stack_item(t, obs, entry);
    let StackItem::Spell { slot, .. } = item else {
        unreachable!("a copy of a spell is a spell");
    };
    let player = side(obs, entry.controller());
    Copied {
        record: ObjectView {
            name: Some(t.def(entry.card()).face_name(entry.face()).to_owned()),
            token: true,
            ..ObjectView::hidden(slot, ZoneName::Stack, player, player)
        },
        item,
    }
}

/// An engine event as the viewer of `obs` is told it. Engine events name objects by slot and
/// carry no identity: anything an event says about a card must come from `obs`, which the
/// information independence test redeals. The table names only an ability's card, public as
/// the stack shows it.
#[allow(clippy::too_many_lines)] // A line or five per event, in one exhaustive match.
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
            slot: ObjectId(object.slot.num()),
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
            source: ObjectId(source.slot.num()),
            to: recipient(obs, to),
            amount,
            combat,
        },
        Event::Draw { .. }
        | Event::Scry { .. }
        | Event::TopOrBottom { .. }
        | Event::LibraryShuffled { .. } => library_event(obs, e),
        Event::DrawCard { .. } => unreachable!("draw slots are grouped with their draw"),
        Event::Reveal { player, slot } => GameEvent::Reveal {
            player: side(player),
            slot: ObjectId(slot.num()),
        },
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
            slot: ObjectId(object.slot.num()),
        },
        Event::Untap { object } => GameEvent::Untap {
            slot: ObjectId(object.slot.num()),
        },
        Event::Mana { .. } => mana_event(obs, e),
        Event::ResolutionPayment { player, ref item } => GameEvent::ResolutionPayment {
            player: side(player),
            item: stack_item(t, obs, item),
        },
        Event::AbilityFinished => GameEvent::AbilityFinished,
        Event::StackArrival { .. } | Event::StackDeparture { .. } => {
            unreachable!("the stack's arrivals and departures are in no view")
        }
        Event::Block { .. } => block_event(obs, e),
        Event::TokensCreated { controller, tokens } => GameEvent::TokensCreated {
            controller: side(controller),
            tokens: slots(obs.in_candidate_order(tokens)),
        },
        Event::Cast {
            spell, controller, ..
        } => GameEvent::Cast {
            slot: ObjectId(spell.slot.num()),
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
            slot: ObjectId(object.slot.num()),
            player: side(player),
        },
        Event::CoinFlip {
            player,
            won,
            source,
        } => GameEvent::CoinFlip {
            player: side(player),
            won,
            source: ObjectId(source.slot.num()),
        },
        Event::Lose { player, .. } => GameEvent::Lose {
            player: side(player),
        },
        Event::Win { player } => GameEvent::Win {
            player: side(player),
        },
        Event::Targeted { .. } | Event::Mutated { .. } => {
            unreachable!("only trigger matching reads a targeting or mutating event")
        }
    }
}
