//! `beryl_cli review <replay.json> [--agent NAME] [--out FILE]`: an app game's transcript,
//! with the agent's search at each of its decisions. The replay file's log answers every
//! decision; the agent, rebuilt from the game's seed as the app built it, searches each of its
//! own again, so its root visits and mean rewards show why it chose as it did. A search that
//! chooses other than the log did is marked: the agent's code changed since the game, the
//! replay drifted, or `--agent` named another agent to search in its place, a registered one
//! or a long-think entrant (`pimc18/omniscient/rollout/tuned/anchor+mid@30M`).

use std::fmt::Write as _;
use std::path::Path;

use beryl_cards::decks::WU_CONTROL;
use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Profile, Replay, Scratch, Seat, Small, State, Step};
use beryl_engine::{apply, next, observe, view};
use beryl_search::{Agent, Budget, GameAgent, agent};
use beryl_session::replay::Entry;
use beryl_session::{ReplayFile, with_stops};

use crate::game;
use crate::transcript::Transcript;

pub fn main(args: &[String]) -> Result<(), String> {
    let usage = || "usage: beryl_cli review <replay.json> [--agent NAME] [--out FILE]".to_owned();
    let [path, flags @ ..] = args else {
        return Err(usage());
    };
    let (mut out, mut searcher) = (None, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else {
            return Err(usage());
        };
        match flag.as_str() {
            "--out" => out = Some(value),
            "--agent" => searcher = Some(value.as_str()),
            _ => return Err(usage()),
        }
    }
    let text = review(Path::new(path), searcher)?;
    if let Some(out) = out {
        std::fs::write(out, text).map_err(|e| format!("{out}: {e}"))
    } else {
        print!("{text}");
        Ok(())
    }
}

/// The agent `name` names for a game from `seed`: a registered agent, or a long-think entrant
/// (`family@budget`) searching on every core.
fn searcher(
    t: &beryl_engine::GameTable,
    name: &str,
    seed: u64,
) -> Result<Box<dyn GameAgent + Send>, String> {
    if let Some(info) = agent(name) {
        return Ok((info.new)(t, seed, Budget::DEFAULT));
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX));
    game::entrant(t, name, seed, threads).unwrap_or_else(|| {
        Err(format!(
            "no agent {name:?}: a registered agent or family@budget"
        ))
    })
}

fn review(path: &Path, searcher_name: Option<&str>) -> Result<String, String> {
    let file = ReplayFile::load(path).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let mirror: Vec<(u8, String)> = WU_CONTROL
        .iter()
        .map(|&(n, name)| (n, name.to_owned()))
        .collect();
    if file.decks.iter().any(|d| *d != mirror) {
        return Err("only WU Control mirror games can be reviewed".to_owned());
    }
    let human = Seat::try_new(file.human).ok_or("no such human seat")?;
    let bot = human.opponent();
    let t = game::mirror();
    let name = searcher_name.unwrap_or(&file.opponent);
    // As `beryl_session`'s `Game::new`: the agent draws from its own stream.
    let mut opponent = searcher(&t, name, SplitMix64::new(file.seed).next_u64())?;
    let mut s = State::new(&t, file.seed, with_stops(Profile::SMART, file.stops, human));
    let mut x = Scratch::recording();
    let names = if human == Seat::P0 {
        ["human", name]
    } else {
        [name, "human"]
    };
    let mut transcript = Transcript::new(names);
    let mut log = file.log.iter();
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => {
                let happened: Vec<Event> = x.events.drain().collect();
                transcript.end(&t, &s, outcome, &happened);
                break;
            }
            Step::Decide(d) => {
                let Some(&entry) = log.next() else {
                    transcript.note("(the record ends here)");
                    break;
                };
                let (Entry::Choice(c) | Entry::Auto(c)) = entry else {
                    return Err(format!("{entry:?}: only answers can be reviewed"));
                };
                let c = Choice(c);
                let happened: Vec<Event> = x.events.drain().collect();
                let search = if d.player == bot {
                    let chose =
                        Agent::<Small>::choose(&mut opponent, observe(&s, &t, &mut x, bot), &d);
                    Some((chose, Agent::<Small>::last_search(&opponent).cloned()))
                } else {
                    None
                };
                transcript.decision(&t, view(&s, &t, &mut x), &d, c, &happened);
                if let Some((chose, Some(search))) = search {
                    let mut line = String::from("    search:");
                    for (i, (&v, &r)) in search.visits.iter().zip(&search.rewards).enumerate() {
                        let mean = if v == 0 { 0.0 } else { r / f64::from(v) };
                        let _ = write!(line, " [{i}] {v} {mean:.3}");
                    }
                    if let Some(a) = search.anchor {
                        let _ = write!(
                            line,
                            "  greedy [{}] lead {:+.3} ± {:.3}{}",
                            a.greedy.0,
                            a.lead,
                            a.error,
                            if a.overrides() { " overridden" } else { "" }
                        );
                    }
                    let _ = write!(
                        line,
                        "  ({} rollouts, {:.1}M steps)",
                        search.rollouts,
                        search.work as f64 / 1e6
                    );
                    if chose != c {
                        let _ = write!(line, "  REPLAY CHOSE [{}]", chose.0);
                    }
                    transcript.note(&line);
                }
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
    }
    Ok(transcript.text().to_owned())
}
