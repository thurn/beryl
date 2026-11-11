//! UI fixtures (decision D11): a seeded game between named agents, or a board of
//! [`crate::boards`] its script plays, played through the app's [`Sessions`] with an agent
//! standing in for the human seat, recorded as the messages the app would have sent the UI up
//! to a named stopping condition.

mod condition;
mod until;
mod view_predicates;

use std::path::Path;
use std::sync::{Arc, mpsc::channel};

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_search::GameAgent;
use beryl_session::ipc::{Fixture, FixtureMessage};
use beryl_session::{
    Prepared, Seat, SessionError, SessionId, Sessions, StartGame, Status, StopsSetting, agents, ipc,
};
use serde::Deserialize;

use crate::boards::{self, Scripted, Shared};

pub(crate) use self::condition::Condition;
use self::until::Until;

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
    /// A seeded game's seed, with `decks`; or none, with `board`.
    seed: Option<u64>,
    /// Each seat's deck: `wu-control` (the mirror) or `fuzz:<seed>` for a fuzz deck.
    decks: Option<[String; 2]>,
    /// The board the game starts from, by its name in [`crate::boards::BOARDS`], in place of
    /// a seeded deal.
    board: Option<String>,
    /// The agent playing each seat of a seeded game. The stopping condition's seat is the
    /// human's. A board's own actions play it.
    agents: Option<[String; 2]>,
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
        let mut messages = record(def).map_err(|e| format!("fixture {}: {e}", def.name))?;
        messages.insert(0, beryl_session::card_records::fixture_message(&messages));
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
    let (sent, updates) = channel();
    let sessions = Sessions::new(None, move |update| {
        // The recording stops listening once it has what it came for.
        let _ = sent.send(update);
    });
    let progress = Shared::default();
    let session = start(def, &sessions, human, &progress)?;
    let mut messages = vec![
        FixtureMessage::response(ipc::LIST_AGENTS, &agents()),
        FixtureMessage::response(ipc::START_GAME, &session),
        // A stops change returns nothing; its view is the open decision's again.
        FixtureMessage::response(ipc::SET_STOPS, &()),
    ];
    // The human's decisions still to record once the stop is reached.
    let mut left = None;
    loop {
        let update = updates.recv();
        if let Some(failure) = boards::lock(&progress).failure.take() {
            return Err(failure);
        }
        let update = update.map_err(|_| "the session ended without a view".to_owned())?;
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
            // A board's actions all lead up to where its recording ends.
            if let Some(action) = boards::lock(&progress).next.iter().flatten().next() {
                return Err(format!("the board's {action:?} was never taken"));
            }
            return Ok(messages);
        }
        if ended {
            let why = boards::lock(&progress).stalled();
            return Err(match why {
                Some(why) => format!("the game ended before {:?}, with {why}", def.until),
                None => format!("the game ended before {:?}", def.until),
            });
        }
        if deciding {
            // The stand-in answered; the UI's `choose` would have returned nothing.
            messages.push(FixtureMessage::response(ipc::CHOOSE, &()));
        }
    }
}

/// Starts the fixture's game in `sessions`: its seeded deal, or its board, whose scripts
/// report to `progress`.
fn start(
    def: &Definition,
    sessions: &Sessions,
    human: Seat,
    progress: &Shared,
) -> Result<SessionId, String> {
    let stops = def.stops.unwrap_or(StopsSetting::SMART);
    let started: Result<SessionId, SessionError> =
        match (&def.board, def.seed, &def.decks, &def.agents) {
            (None, Some(seed), Some(decks), Some(agents)) => sessions.start_game(
                &StartGame {
                    seed,
                    decks: [deck(&decks[0])?, deck(&decks[1])?],
                    human,
                    opponent: agents[human.opponent().index()].clone(),
                    stops,
                },
                Some(&agents[human.index()]),
            ),
            (Some(name), None, None, None) => {
                let board = boards::named(name)?;
                let (table, state) =
                    beryl_scenario::position(&CARDS, board).map_err(|e| e.to_string())?;
                let agent = |seat: Seat| {
                    Scripted::new(board, seat, &table, Arc::clone(progress))
                        .map(|a| Box::new(a) as Box<dyn GameAgent + Send>)
                };
                sessions.start_prepared(Prepared {
                    opponent: agent(human.opponent())?,
                    stand_in: agent(human)?,
                    scripted: true,
                    hand_over: None,
                    table,
                    state,
                    human,
                    stops,
                })
            }
            _ => {
                return Err("a fixture has a seed, decks and agents, or a board".to_owned());
            }
        };
    started.map_err(|e| e.to_string())
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

fn number<T: std::str::FromStr>(text: &str, n: &str) -> Result<T, String> {
    n.parse().map_err(|_| format!("{text:?}: bad number {n:?}"))
}

#[cfg(test)]
mod tests {
    use beryl_session::GameUpdate;
    use beryl_session::protocol::{Kind, Step, ZoneName};

    use super::condition::Mark;
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
            Condition::parse("playable Sink into Stupor"),
            Ok(Condition::Playable("Sink into Stupor".to_owned()))
        );
        assert_eq!(
            Condition::parse("abilities of The Wandering Emperor"),
            Ok(Condition::Abilities("The Wandering Emperor".to_owned()))
        );
        assert_eq!(
            Condition::parse("namesake candidates"),
            Ok(Condition::NamesakeCandidates)
        );
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
            Condition::parse("exiled with a permanent"),
            Ok(Condition::ExiledWith)
        );
        assert_eq!(
            Condition::parse("attached across the table"),
            Ok(Condition::AttachedAcross)
        );
        assert_eq!(
            Condition::parse("damage beside a twin"),
            Ok(Condition::Marked(Mark::Damage))
        );
        assert_eq!(
            Condition::parse("castable from exile"),
            Ok(Condition::CastableFrom(ZoneName::Exile))
        );
        assert_eq!(
            Condition::parse("known card in the opponent's hand"),
            Ok(Condition::KnownOpponentsHand)
        );
        assert_eq!(
            Condition::parse("ability targeting beside its source"),
            Ok(Condition::NeighbourTarget)
        );
        assert_eq!(Condition::parse("token twins"), Ok(Condition::TokenTwins));
        assert_eq!(Condition::parse("emblems a side"), Ok(Condition::Emblems));
        assert_eq!(
            Condition::parse("changed token"),
            Ok(Condition::ChangedToken)
        );
        assert_eq!(
            Condition::parse("token beside its card"),
            Ok(Condition::TokenBesideCard)
        );
        assert_eq!(Condition::parse("stolen permanent"), Ok(Condition::Stolen));
        assert_eq!(
            Condition::parse("mana floating"),
            Ok(Condition::ManaFloating)
        );
        assert_eq!(Condition::parse("X on the stack"), Ok(Condition::XOnStack));
        assert_eq!(
            Condition::parse("8 nonland permanents a side"),
            Ok(Condition::Crowded(8))
        );
        assert_eq!(
            Condition::parse("spell on the stack"),
            Ok(Condition::SpellOnStack)
        );
        assert!(Condition::parse("many nonland permanents a side").is_err());
        assert!(Condition::parse("step lunch").is_err());
        assert!(Condition::parse("raining").is_err());
    }

    /// Every board fixture the UI records still plays out to its stop, in a fraction of a
    /// second, so a change that breaks one fails here rather than in the UI's checks.
    #[test]
    fn every_board_fixture_records() {
        // Read at run time: a test binary from the shared target directory may have been
        // built in a worktree that no longer exists.
        let path = Path::new(
            &std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("../../../ui/fixtures.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let defs: Definitions = serde_json::from_str(&text).unwrap();
        let boards: Vec<&Definition> = defs.fixtures.iter().filter(|d| d.board.is_some()).collect();
        assert!(!boards.is_empty());
        for def in boards {
            record(def).unwrap_or_else(|e| panic!("fixture {}: {e}", def.name));
        }
    }

    #[test]
    fn a_recording_stops_at_its_condition() {
        let def = Definition {
            name: "t".to_owned(),
            seed: Some(4),
            decks: Some(["fuzz:1".to_owned(), "fuzz:2".to_owned()]),
            board: None,
            agents: Some(["random".to_owned(), "random".to_owned()]),
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
