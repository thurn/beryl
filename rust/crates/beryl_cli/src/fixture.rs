//! UI fixtures (decision D11): a seeded game between named agents, played through the app's
//! [`Sessions`] with an agent standing in for the human seat, recorded as the messages the
//! app would have sent the UI up to a named stopping condition.

use std::path::Path;
use std::sync::mpsc::channel;

use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_session::ipc::{self, Fixture, FixtureMessage};
use beryl_session::protocol::{Candidate, Combat, Kind, Side, StackItem, Step};
use beryl_session::{PlayerView, Seat, Sessions, StartGame, Status, StopsSetting, agents};
use serde::Deserialize;
use serde_json::json;

/// The definitions file.
#[derive(Debug, Deserialize)]
struct Definitions {
    fixtures: Vec<Definition>,
}

/// One fixture: the game to play and where to stop.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    name: String,
    seed: u64,
    /// Each seat's deck: `wu-control`, or `fuzz:<seed>` for a fuzz deck.
    decks: [String; 2],
    /// The agent playing each seat. The stopping condition's seat is the human's.
    agents: [String; 2],
    stops: Option<StopsSetting>,
    until: String,
    /// What else must hold of the view the recording stops at (see [`Condition::parse`]).
    #[serde(default, rename = "where")]
    conditions: Vec<String>,
}

/// Where a recording stops: the human's first decision of a kind, from a turn on, or the end
/// of the game.
#[derive(Debug, PartialEq)]
enum Until {
    First { kind: Kind, seat: Seat, turn: u16 },
    GameOver,
}

impl Until {
    /// Parses "first <kind> decision for seat <n>", optionally followed by "from turn <t>",
    /// with the kind as the protocol names it ("target", "chooseUpTo"), or "game over".
    fn parse(text: &str) -> Result<Until, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (words, turn) = match words.as_slice() {
            [first @ .., "from", "turn", turn] => (
                first,
                turn.parse()
                    .map_err(|_| format!("{text:?}: no turn {turn:?}"))?,
            ),
            all => (all, 0),
        };
        match words {
            ["game", "over"] if turn == 0 => Ok(Until::GameOver),
            ["first", kind, "decision", "for", "seat", seat] => Ok(Until::First {
                turn,
                kind: serde_json::from_value(json!(kind))
                    .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                seat: seat
                    .parse()
                    .ok()
                    .and_then(Seat::try_new)
                    .ok_or_else(|| format!("no seat {seat:?}"))?,
            }),
            _ => Err(format!(
                "{text:?}: expected \"first <kind> decision for seat <n> [from turn <t>]\" or \
                 \"game over\""
            )),
        }
    }

    fn seat(&self) -> Seat {
        match self {
            Until::First { seat, .. } => *seat,
            Until::GameOver => Seat::P0,
        }
    }

    fn reached(&self, view: &PlayerView) -> bool {
        match self {
            Until::First { kind, turn, .. } => {
                view.turn.number >= *turn
                    && matches!(
                        &view.status,
                        Status::Deciding { decision } if decision.kind == *kind
                    )
            }
            Until::GameOver => matches!(view.status, Status::Over { .. }),
        }
    }
}

/// A condition on the view a recording stops at, so a fixture can stop at a state rather
/// than only at a kind of decision.
#[derive(Debug, PartialEq)]
enum Condition {
    /// The turn is at this step.
    Step(Step),
    /// Some creature is attacking.
    Attacking,
    /// Some creature is blocking.
    Blocking,
    /// The top of the stack is a triggered ability with a target.
    TargetedTrigger,
    /// The top of the stack is a spell the opponent controls.
    OpponentsSpell,
    /// The human may cast a card of this name.
    Castable(String),
    /// The open decision is asked by the resolution of a spell of this name or an ability of
    /// a card of this name.
    Resolving(String),
}

impl Condition {
    /// Parses "step <step>" (as the protocol names steps: "declareBlockers"), "attacking",
    /// "blocking", "targeted trigger on the stack", "opponent's spell on the stack" or
    /// "castable <card name>" or "resolving <card name>".
    fn parse(text: &str) -> Result<Condition, String> {
        let condition = match text {
            "attacking" => Condition::Attacking,
            "blocking" => Condition::Blocking,
            "targeted trigger on the stack" => Condition::TargetedTrigger,
            "opponent's spell on the stack" => Condition::OpponentsSpell,
            _ => {
                if let Some(step) = text.strip_prefix("step ") {
                    Condition::Step(
                        serde_json::from_value(json!(step))
                            .map_err(|_| format!("no step is named {step:?}"))?,
                    )
                } else if let Some(name) = text.strip_prefix("castable ") {
                    Condition::Castable(name.to_owned())
                } else if let Some(name) = text.strip_prefix("resolving ") {
                    Condition::Resolving(name.to_owned())
                } else {
                    return Err(format!("{text:?}: no such condition"));
                }
            }
        };
        Ok(condition)
    }

    fn holds(&self, view: &PlayerView) -> bool {
        let name = |slot: u8| {
            view.objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.as_deref())
        };
        match self {
            Condition::Step(step) => view.turn.step == *step,
            Condition::Attacking => view
                .objects
                .iter()
                .any(|o| matches!(o.combat, Some(Combat::Attacking { .. }))),
            Condition::Blocking => view
                .objects
                .iter()
                .any(|o| matches!(o.combat, Some(Combat::Blocking { .. }))),
            Condition::TargetedTrigger => matches!(
                view.stack.last(),
                Some(StackItem::Ability { triggered: true, targets, .. }) if !targets.is_empty()
            ),
            Condition::OpponentsSpell => matches!(
                view.stack.last(),
                Some(StackItem::Spell { slot, .. })
                    if view.objects.iter().any(|o| o.slot == *slot && o.controller == Side::Opponent)
            ),
            Condition::Castable(card) => matches!(
                &view.status,
                Status::Deciding { decision } if decision.candidates.iter().any(|c| matches!(
                    c.option,
                    Candidate::Cast { slot, .. } if name(slot) == Some(card.as_str())
                ))
            ),
            Condition::Resolving(card) => match &view.status {
                Status::Deciding { decision } => match &decision.resolving {
                    Some(StackItem::Spell { slot, .. }) => name(*slot) == Some(card.as_str()),
                    Some(StackItem::Ability { name, .. }) => name == card,
                    None => false,
                },
                _ => false,
            },
        }
    }
}

/// Records the fixtures in `definitions` named in `only`, or all of them, into `out`.
pub fn record_all(definitions: &Path, out: &Path, only: &[String]) -> Result<(), String> {
    let text = std::fs::read_to_string(definitions)
        .map_err(|e| format!("{}: {e}", definitions.display()))?;
    let defs: Definitions =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", definitions.display()))?;
    if let Some(name) = only
        .iter()
        .find(|n| !defs.fixtures.iter().any(|f| &f.name == *n))
    {
        return Err(format!("no fixture is named {name:?}"));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for def in &defs.fixtures {
        if !only.is_empty() && !only.contains(&def.name) {
            continue;
        }
        let messages = record(def).map_err(|e| format!("fixture {}: {e}", def.name))?;
        let path = out.join(format!("{}.json", def.name));
        let json = serde_json::to_string(&Fixture { messages }).expect("a fixture serializes");
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

fn record(def: &Definition) -> Result<Vec<FixtureMessage>, String> {
    let until = Until::parse(&def.until)?;
    let conditions = def
        .conditions
        .iter()
        .map(|c| Condition::parse(c))
        .collect::<Result<Vec<_>, _>>()?;
    let human = until.seat();
    let start = StartGame {
        seed: def.seed,
        decks: [deck(&def.decks[0])?, deck(&def.decks[1])?],
        human,
        opponent: def.agents[human.opponent().index()].clone(),
        stops: def.stops.unwrap_or(StopsSetting::Smart),
    };
    let (sent, updates) = channel();
    let sessions = Sessions::new(None, move |update| {
        // The recording stops listening once it has what it came for.
        let _ = sent.send(update);
    });
    let session = sessions
        .start_game(&start, Some(&def.agents[human.index()]))
        .map_err(|e| e.to_string())?;
    let mut messages = vec![
        FixtureMessage::response(ipc::LIST_AGENTS, &agents()),
        FixtureMessage::response(ipc::START_GAME, &session),
    ];
    loop {
        let update = updates
            .recv()
            .map_err(|_| "the session ended without a view".to_owned())?;
        let reached =
            until.reached(&update.view) && conditions.iter().all(|c| c.holds(&update.view));
        let deciding = matches!(update.view.status, Status::Deciding { .. });
        messages.push(FixtureMessage::game_update(&update));
        if reached {
            return Ok(messages);
        }
        if !deciding {
            return Err(format!("the game ended before {:?}", def.until));
        }
        // The stand-in answered; the UI's `choose` would have returned nothing.
        messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
    }
}

fn deck(name: &str) -> Result<Vec<(u8, String)>, String> {
    let list = if name == "wu-control" {
        WU_CONTROL.to_vec()
    } else if let Some(seed) = name.strip_prefix("fuzz:") {
        fuzz_deck(
            seed.parse()
                .map_err(|_| format!("bad fuzz seed {seed:?}"))?,
        )
    } else {
        return Err(format!("no deck is named {name:?}"));
    };
    Ok(list.into_iter().map(|(n, c)| (n, c.to_owned())).collect())
}

#[cfg(test)]
mod tests {
    use beryl_session::GameUpdate;

    use super::*;

    #[test]
    fn stopping_conditions_parse() {
        assert_eq!(
            Until::parse("first chooseUpTo decision for seat 1"),
            Ok(Until::First {
                kind: Kind::ChooseUpTo,
                seat: Seat::P1,
                turn: 0,
            })
        );
        assert_eq!(
            Until::parse("first priority decision for seat 0 from turn 9"),
            Ok(Until::First {
                kind: Kind::Priority,
                seat: Seat::P0,
                turn: 9,
            })
        );
        assert!(Until::parse("game over from turn 3").is_err());
        assert_eq!(Until::parse("game over"), Ok(Until::GameOver));
        assert!(Until::parse("first scrying decision for seat 0").is_err());
        assert!(Until::parse("first target decision for seat 2").is_err());
    }

    #[test]
    fn conditions_parse() {
        assert_eq!(
            Condition::parse("step declareBlockers"),
            Ok(Condition::Step(Step::DeclareBlockers))
        );
        assert_eq!(
            Condition::parse("castable Mana Drain"),
            Ok(Condition::Castable("Mana Drain".to_owned()))
        );
        assert_eq!(Condition::parse("blocking"), Ok(Condition::Blocking));
        assert_eq!(
            Condition::parse("resolving Mana Tithe"),
            Ok(Condition::Resolving("Mana Tithe".to_owned()))
        );
        assert!(Condition::parse("step lunch").is_err());
        assert!(Condition::parse("raining").is_err());
    }

    #[test]
    fn a_recording_stops_at_its_condition() {
        let def = Definition {
            name: "t".to_owned(),
            seed: 4,
            decks: ["fuzz:1".to_owned(), "fuzz:2".to_owned()],
            agents: ["random".to_owned(), "random".to_owned()],
            stops: None,
            until: "first target decision for seat 1".to_owned(),
            conditions: vec![],
        };
        let messages = record(&def).unwrap();
        let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
            panic!("a recording ends on a view");
        };
        let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
        let Status::Deciding { decision } = update.view.status else {
            panic!("a recording stops at a decision");
        };
        assert_eq!(decision.kind, Kind::Target);
    }
}
