//! UI fixtures (decision D11): a seeded game between named agents, played through a
//! [`Game`] whose human seat an agent plays, recorded as the messages the app would have sent
//! the UI up to a named stopping condition.

use std::path::Path;

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::rng::SplitMix64;
use beryl_session::protocol::Kind;
use beryl_session::{Game, GameUpdate, PlayerView, StartGame, StopsSetting, agents};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The session number fixtures report: one game per fixture.
const SESSION: u32 = 1;

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
}

/// One message of the stream, as `FixtureMessage` in `ui/src/ipc.ts`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Message {
    Response { command: &'static str, value: Value },
    Event { event: &'static str, payload: Value },
}

/// Where a recording stops: the human's first decision of a kind, or the end of the game.
#[derive(Debug, PartialEq)]
enum Until {
    First { kind: Kind, seat: u8 },
    GameOver,
}

impl Until {
    /// Parses "first <kind> decision for seat <n>", with the kind as the protocol names it
    /// ("target", "chooseUpTo"), or "game over".
    fn parse(text: &str) -> Result<Until, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        match words.as_slice() {
            ["game", "over"] => Ok(Until::GameOver),
            ["first", kind, "decision", "for", "seat", seat] => Ok(Until::First {
                kind: serde_json::from_value(json!(kind))
                    .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                seat: seat
                    .parse()
                    .ok()
                    .filter(|&s| s < 2)
                    .ok_or_else(|| format!("no seat {seat:?}"))?,
            }),
            _ => Err(format!(
                "{text:?}: expected \"first <kind> decision for seat <n>\" or \"game over\""
            )),
        }
    }

    fn seat(&self) -> u8 {
        match self {
            Until::First { seat, .. } => *seat,
            Until::GameOver => 0,
        }
    }

    fn reached(&self, view: &PlayerView) -> bool {
        match self {
            Until::First { kind, .. } => view.decision.as_ref().is_some_and(|d| d.kind == *kind),
            Until::GameOver => view.outcome.is_some(),
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
        let json =
            serde_json::to_string(&json!({ "messages": messages })).expect("a fixture serializes");
        std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{}", path.display());
    }
    Ok(())
}

fn record(def: &Definition) -> Result<Vec<Message>, String> {
    let until = Until::parse(&def.until)?;
    let human = until.seat();
    let opponent = 1 - human;
    let agent = |seat: u8| {
        let name = &def.agents[usize::from(seat)];
        beryl_search::agent(name).ok_or_else(|| format!("no agent is named {name:?}"))
    };
    let start = StartGame {
        seed: def.seed,
        decks: [deck(&def.decks[0])?, deck(&def.decks[1])?],
        human,
        opponent: agent(opponent)?.name.to_owned(),
        stops: def.stops.unwrap_or(StopsSetting::Smart),
    };
    // The human's seat draws from its own stream, apart from the game's and the opponent's.
    let mut stand_in = (agent(human)?.new)(SplitMix64::new(def.seed ^ 0x9e37_79b9).next_u64());
    let mut game = Game::new(&CARDS, &start).map_err(|e| e.to_string())?;
    let mut messages = vec![
        response("list_agents", json!(agents())),
        response("start_game", json!(SESSION)),
    ];
    let mut view = game.begin();
    loop {
        let reached = until.reached(&view);
        let over = view.outcome.is_some();
        messages.push(Message::Event {
            event: "game-update",
            payload: json!(GameUpdate {
                session: SESSION,
                view,
            }),
        });
        if reached {
            return Ok(messages);
        }
        if over {
            return Err(format!("the game ended before {:?}", def.until));
        }
        view = game
            .choose_for_human(stand_in.as_mut())
            .map_err(|e| e.to_string())?;
        messages.push(response("choose", Value::Null));
    }
}

fn response(command: &'static str, value: Value) -> Message {
    Message::Response { command, value }
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
    use super::*;

    #[test]
    fn stopping_conditions_parse() {
        assert_eq!(
            Until::parse("first chooseUpTo decision for seat 1"),
            Ok(Until::First {
                kind: Kind::ChooseUpTo,
                seat: 1
            })
        );
        assert_eq!(Until::parse("game over"), Ok(Until::GameOver));
        assert!(Until::parse("first scrying decision for seat 0").is_err());
        assert!(Until::parse("first target decision for seat 2").is_err());
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
        };
        let messages = record(&def).unwrap();
        let Some(Message::Event { payload, .. }) = messages.last() else {
            panic!("a recording ends on a view");
        };
        let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
        let d = update.view.decision.unwrap();
        assert_eq!((d.kind, d.player, update.view.viewer), (Kind::Target, 1, 1));
    }
}
