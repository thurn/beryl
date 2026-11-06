//! UI fixtures (decision D11): a seeded game between named agents, played through the app's
//! [`Sessions`] with an agent standing in for the human seat, recorded as the messages the
//! app would have sent the UI up to a named stopping condition.

use std::path::Path;
use std::sync::mpsc::channel;

use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_session::ipc::{self, Fixture, FixtureMessage};
use beryl_session::protocol::{
    Candidate, Combat, Kind, Side, StackItem, Step, TargetRef, ZoneName,
};
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
    /// How many more of the human's decisions to record past the stop, as the stand-in plays
    /// on: what the decision there led to.
    #[serde(default)]
    after: usize,
}

/// Where a recording stops: the human's first decision of a kind, or first halt
/// (`DecisionView::halt`), from a turn on, or the end of the game.
#[derive(Debug, PartialEq)]
enum Until {
    First { kind: Kind, seat: Seat, turn: u16 },
    Halt { seat: Seat, turn: u16 },
    GameOver,
}

impl Until {
    /// Parses "first <kind> decision for seat <n>" or "first halt for seat <n>", optionally
    /// followed by "from turn <t>", with the kind as the protocol names it ("target",
    /// "chooseUpTo"), or "game over". A decision of a kind is the engine's, never a halt.
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
        let seat = |seat: &str| {
            seat.parse()
                .ok()
                .and_then(Seat::try_new)
                .ok_or_else(|| format!("no seat {seat:?}"))
        };
        match words {
            ["game", "over"] if turn == 0 => Ok(Until::GameOver),
            ["first", kind, "decision", "for", "seat", n] => Ok(Until::First {
                turn,
                kind: serde_json::from_value(json!(kind))
                    .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                seat: seat(n)?,
            }),
            ["first", "halt", "for", "seat", n] => Ok(Until::Halt {
                turn,
                seat: seat(n)?,
            }),
            _ => Err(format!(
                "{text:?}: expected \"first <kind> decision for seat <n> [from turn <t>]\", \
                 \"first halt for seat <n> [from turn <t>]\" or \"game over\""
            )),
        }
    }

    fn seat(&self) -> Seat {
        match self {
            Until::First { seat, .. } | Until::Halt { seat, .. } => *seat,
            Until::GameOver => Seat::P0,
        }
    }

    fn reached(&self, view: &PlayerView) -> bool {
        let decision = match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        };
        match self {
            Until::First { kind, turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.kind == *kind && !d.halt)
            }
            Until::Halt { turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.halt)
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
    /// The top of the stack is a spell that targets a permanent.
    TargetedSpell,
    /// The top of the stack targets a player.
    PlayerTarget,
    /// The top of the stack is an ability of a permanent that targets another permanent its
    /// controller controls, which the table sets in the same row.
    NeighbourTarget,
    /// The top of the stack is a spell the opponent controls.
    OpponentsSpell,
    /// The human may cast a card of this name.
    Castable(String),
    /// The open decision is asked by the resolution of a spell of this name or an ability of
    /// a card of this name.
    Resolving(String),
    /// The open decision is asked by the casting of a spell of this name: its X, a target or
    /// its payment.
    Casting(String),
    /// The human may play or cast a card from their hand of which they hold another just like
    /// it, which the engine offers as one candidate for both (architecture §7.2).
    PlayablePair,
    /// A card lies in exile beneath the permanent that exiled it until it leaves (CR 610.3).
    ExiledBeneath,
    /// An Aura or Equipment one player controls is attached to a permanent the other
    /// controls.
    AttachedAcross,
}

impl Condition {
    /// Parses "step <step>" (as the protocol names steps: "declareBlockers"), "attacking",
    /// "blocking", "targeted trigger on the stack", "targeted spell on the stack", "player
    /// targeted on the stack", "ability targeting beside its source", "opponent's spell on
    /// the stack", "castable <card name>", "resolving <card name>", "casting <card name>",
    /// "playable pair in hand", "exiled beneath a permanent" or "attached across the table".
    fn parse(text: &str) -> Result<Condition, String> {
        let condition = match text {
            "attacking" => Condition::Attacking,
            "blocking" => Condition::Blocking,
            "targeted trigger on the stack" => Condition::TargetedTrigger,
            "targeted spell on the stack" => Condition::TargetedSpell,
            "player targeted on the stack" => Condition::PlayerTarget,
            "ability targeting beside its source" => Condition::NeighbourTarget,
            "opponent's spell on the stack" => Condition::OpponentsSpell,
            "playable pair in hand" => Condition::PlayablePair,
            "exiled beneath a permanent" => Condition::ExiledBeneath,
            "attached across the table" => Condition::AttachedAcross,
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
                } else if let Some(name) = text.strip_prefix("casting ") {
                    Condition::Casting(name.to_owned())
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
            Condition::TargetedSpell => matches!(
                view.stack.last(),
                Some(StackItem::Spell { targets, .. })
                    if targets.iter().any(|t| matches!(t, TargetRef::Object { .. }))
            ),
            Condition::PlayerTarget => view.stack.last().is_some_and(|item| {
                let (StackItem::Spell { targets, .. } | StackItem::Ability { targets, .. }) = item;
                targets
                    .iter()
                    .any(|t| matches!(t, TargetRef::Player { .. }))
            }),
            Condition::NeighbourTarget => targets_beside_source(view),
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
            Condition::Casting(card) => match &view.status {
                Status::Deciding { decision } => {
                    decision.resolving.is_none()
                        && decision.source.is_some_and(|slot| {
                            name(slot) == Some(card.as_str())
                                && view
                                    .objects
                                    .iter()
                                    .any(|o| o.slot == slot && o.zone == ZoneName::Stack)
                        })
                }
                _ => false,
            },
            Condition::PlayablePair => match &view.status {
                Status::Deciding { decision } => decision.candidates.iter().any(|c| {
                    let (Candidate::Cast { slot, .. } | Candidate::PlayLand { slot }) = c.option
                    else {
                        return false;
                    };
                    let held = |s: u8| {
                        view.objects
                            .iter()
                            .any(|o| o.slot == s && o.zone == ZoneName::Hand)
                    };
                    held(slot)
                        && view.objects.iter().any(|o| {
                            o.slot != slot
                                && o.zone == ZoneName::Hand
                                && o.owner == Side::You
                                && o.name.is_some()
                                && o.name.as_deref() == name(slot)
                        })
                }),
                _ => false,
            },
            Condition::ExiledBeneath => view.objects.iter().any(|o| {
                o.zone == ZoneName::Exile
                    && o.exiled_by.is_some_and(|host| {
                        view.objects
                            .iter()
                            .any(|h| h.slot == host && h.zone == ZoneName::Battlefield)
                    })
            }),
            Condition::AttachedAcross => attached_across(view),
        }
    }
}

/// Whether a permanent one player controls is attached to a permanent the other controls.
fn attached_across(view: &PlayerView) -> bool {
    view.objects.iter().any(|o| {
        o.zone == ZoneName::Battlefield
            && o.attached_to.is_some_and(|host| {
                view.objects.iter().any(|h| {
                    h.slot == host
                        && h.zone == ZoneName::Battlefield
                        && h.controller != o.controller
                })
            })
    })
}

/// Whether the top of the stack is an ability of a permanent that targets another permanent
/// its controller controls.
fn targets_beside_source(view: &PlayerView) -> bool {
    let Some(StackItem::Ability {
        source: Some(source),
        targets,
        ..
    }) = view.stack.last()
    else {
        return false;
    };
    let permanent = |slot: u8| {
        view.objects
            .iter()
            .find(|o| o.slot == slot && o.zone == ZoneName::Battlefield)
    };
    permanent(*source).is_some_and(|s| {
        targets.iter().any(|t| {
            matches!(t, TargetRef::Object { slot } if *slot != *source
                && permanent(*slot).is_some_and(|o| o.controller == s.controller))
        })
    })
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
    // The human's decisions still to record once the stop is reached.
    let mut left = None;
    loop {
        let update = updates
            .recv()
            .map_err(|_| "the session ended without a view".to_owned())?;
        let reached = left.is_none()
            && until.reached(&update.view)
            && conditions.iter().all(|c| c.holds(&update.view));
        let status = &update.view.status;
        let (deciding, ended) = (
            matches!(status, Status::Deciding { .. }),
            matches!(status, Status::Over { .. } | Status::Stopped),
        );
        messages.push(FixtureMessage::game_update(&update));
        if reached {
            left = Some(def.after);
        } else if let Some(n) = left.filter(|_| deciding) {
            left = Some(n - 1);
        }
        if left == Some(0) || (ended && left.is_some()) {
            return Ok(messages);
        }
        if ended {
            return Err(format!("the game ended before {:?}", def.until));
        }
        if deciding {
            // The stand-in answered; the UI's `choose` would have returned nothing.
            messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
        }
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
        assert_eq!(
            Until::parse("first halt for seat 1 from turn 4"),
            Ok(Until::Halt {
                seat: Seat::P1,
                turn: 4,
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
        assert_eq!(
            Condition::parse("casting Thieving Skydiver"),
            Ok(Condition::Casting("Thieving Skydiver".to_owned()))
        );
        assert_eq!(
            Condition::parse("playable pair in hand"),
            Ok(Condition::PlayablePair)
        );
        assert_eq!(
            Condition::parse("exiled beneath a permanent"),
            Ok(Condition::ExiledBeneath)
        );
        assert_eq!(
            Condition::parse("attached across the table"),
            Ok(Condition::AttachedAcross)
        );
        assert_eq!(
            Condition::parse("ability targeting beside its source"),
            Ok(Condition::NeighbourTarget)
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
            after: 0,
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
