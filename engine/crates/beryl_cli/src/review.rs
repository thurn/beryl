//! `beryl_cli review <replay.json> [--agent NAME] [--turn N] [--out FILE]`: an app game's transcript,
//! with an agent's search at each of the opponent's decisions. The replay file's log answers
//! every decision. By default `greedy` stands in for the opponent, which does not search, so
//! the transcript costs seconds. `--agent recorded` rebuilds the game's own agent from its seed
//! as the app built it, and `--agent NAME` another, a registered one or a long-think entrant
//! (`pimc18/omniscient/rollout/tuned/anchor+mid@30M`): it searches each of the opponent's
//! decisions again, so its root visits and mean rewards show why it chose as it did. A search
//! that chooses other than the log did is marked: the agent's code changed since the game, the
//! replay drifted, or the agent is not the one that played. `--turn N` searches only turn
//! `N`'s decisions and ends the transcript there. Each search is asked by the decision's
//! index among the agent's own, counted over the decisions it skips too, so it samples as the
//! game's agent did there, but without the trees a search may keep from the decision before.

use std::fmt::Write as _;
use std::path::Path;

use beryl_engine::rng::SplitMix64;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Choice, Seat, Small};
use beryl_engine::{observe, view};
use beryl_search::{Agent, AgentDecision, Budget, GameAgent, Search, find_agent};
use beryl_session::ReplayFile;

use crate::Catalog;
use crate::game;
use crate::transcript::Transcript;
use crate::walk::{Walk, Walked};

pub fn main(catalog: &'static Catalog, args: &[String]) -> Result<(), String> {
    let usage = || {
        "usage: beryl_cli review <replay.json> [--agent NAME] [--turn N] [--out FILE]".to_owned()
    };
    let [path, flags @ ..] = args else {
        return Err(usage());
    };
    let (mut out, mut searcher, mut only) = (None, None, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else {
            return Err(usage());
        };
        match flag.as_str() {
            "--out" => out = Some(value),
            "--agent" => searcher = Some(value.as_str()),
            "--turn" => only = Some(value.parse::<u16>().map_err(|_| usage())?),
            _ => return Err(usage()),
        }
    }
    let text = review(catalog, Path::new(path), searcher, only)?;
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
    if let Some(info) = find_agent(name)? {
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

/// The agent `--agent` names: `greedy` by default, which does not search, and the game's own
/// for `recorded`.
fn agent_name<'a>(file: &'a ReplayFile, searcher_name: Option<&'a str>) -> &'a str {
    match searcher_name {
        None => "greedy",
        Some("recorded") => &file.opponent,
        Some(name) => name,
    }
}

/// With `only`, the agent searches that turn's decisions alone and the transcript ends with it.
pub fn review(
    catalog: &'static Catalog,
    path: &Path,
    searcher_name: Option<&str>,
    only: Option<u16>,
) -> Result<String, String> {
    let file =
        ReplayFile::load(path, catalog.cards).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let mut walk = Walk::of_file(catalog, &file)?;
    let bot = walk.human.opponent();
    let name = agent_name(&file, searcher_name);
    // As `beryl_session`'s `Game::new`: the agent draws from its own stream.
    let mut opponent = searcher(&walk.t, name, SplitMix64::new(file.seed).next_u64())?;
    // The answers are the recorded agent's, whichever agent searches beside them.
    let recorded = file.opponent.as_str();
    let names = if walk.human == Seat::P0 {
        ["human", recorded]
    } else {
        [recorded, "human"]
    };
    let mut transcript = Transcript::new(names);
    // The agent's own decisions, searched or not, so each is searched from the seed the
    // recorded agent had there.
    let mut own = AgentDecision(0);
    loop {
        match walk.next()? {
            Walked::Over { outcome, happened } => {
                transcript.end(&walk.t, &walk.s, outcome, &happened);
                break;
            }
            Walked::Undone => transcript.note("(the human takes back an answer)"),
            Walked::Conceded => {
                transcript.note("(the human conceded)");
                break;
            }
            Walked::Ends => {
                transcript.note("(the record ends here)");
                break;
            }
            Walked::Decide { d, c, happened } => {
                let turn = walk.s.turn().number();
                if only.is_some_and(|n| turn > n) {
                    transcript.note("(the review stops after the turn it was asked for)");
                    break;
                }
                let (t, s, x) = (&walk.t, &walk.s, &mut walk.x);
                let at = if d.decider == bot {
                    Some(own.take())
                } else {
                    None
                };
                let search = if let Some(at) = at
                    && only.is_none_or(|n| turn == n)
                {
                    let chose =
                        Agent::<Small>::choose(&mut opponent, observe(s, t, x, bot), &d, at);
                    Some((chose, Agent::<Small>::last_search(&opponent).cloned()))
                } else {
                    None
                };
                transcript.decision(t, view(s, t, x), &d, c, &happened);
                if let Some((chose, Some(search))) = search {
                    transcript.note(&search_line(&search, chose, c));
                }
                walk.apply(c);
            }
        }
    }
    Ok(transcript.text().to_owned())
}

/// A search's root candidates, visits and mean reward each, its check against greedy's
/// answer, and whether it chose other than the game's `logged` answer.
fn search_line(search: &Search, chose: Choice, logged: Choice) -> String {
    let mut line = String::from("    search:");
    for (i, (&v, &mean)) in search.visits.iter().zip(&search.means).enumerate() {
        let _ = write!(line, " [{i}] {v} {:.3}", mean.unwrap_or(0.0));
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
    if chose != logged {
        let _ = write!(line, "  REPLAY CHOSE [{}]", chose.0);
    }
    line
}
