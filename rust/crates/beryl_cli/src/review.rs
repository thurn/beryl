//! `beryl_cli review <replay.json> [--agent NAME] [--turn N] [--out FILE]`: an app game's transcript,
//! with an agent's search at each of the opponent's decisions. The replay file's log answers
//! every decision. By default `greedy` stands in for the opponent, which does not search, so
//! the transcript costs seconds. `--agent recorded` rebuilds the game's own agent from its seed
//! as the app built it, and `--agent NAME` another, a registered one or a long-think entrant
//! (`pimc18/omniscient/rollout/tuned/anchor+mid@30M`): it searches each of the opponent's
//! decisions again, so its root visits and mean rewards show why it chose as it did. A search
//! that chooses other than the log did is marked: the agent's code changed since the game, the
//! replay drifted, or the agent is not the one that played. `--turn N` searches only turn
//! `N`'s decisions and ends the transcript there; skipping the searches before it moves the
//! agent's random stream, so its searches sample afresh rather than repeat the game's.

use std::fmt::Write as _;
use std::path::Path;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Seat, Small};
use beryl_engine::{observe, view};
use beryl_search::{Agent, Budget, GameAgent, Search, agent};
use beryl_session::ReplayFile;

use crate::game;
use crate::transcript::Transcript;
use crate::walk::{Walk, Walked};

pub fn main(args: &[String]) -> Result<(), String> {
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
    let text = review(Path::new(path), searcher, only)?;
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
fn review(path: &Path, searcher_name: Option<&str>, only: Option<u16>) -> Result<String, String> {
    let file = ReplayFile::load(path).map_err(|e| format!("{}: {e:?}", path.display()))?;
    let mut walk = Walk::of_file(&file)?;
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
                let turn = walk.s.turn().number;
                if only.is_some_and(|n| turn > n) {
                    transcript.note("(the review stops after the turn it was asked for)");
                    break;
                }
                let (t, s, x) = (&walk.t, &walk.s, &mut walk.x);
                let search = if d.player == bot && only.is_none_or(|n| turn == n) {
                    let chose = Agent::<Small>::choose(&mut opponent, observe(s, t, x, bot), &d);
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

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_cards::decks::WU_CONTROL;
    use beryl_session::protocol::{Candidate, DecisionView, Kind};
    use beryl_session::replay::Entry;
    use beryl_session::{
        CandidateIndex, Game, PlayerView, SessionId, StartGame, Status, StopsSetting,
    };

    use super::*;

    /// The view at the human's next decision or the end.
    fn to_human(game: &mut Game) -> PlayerView {
        let mut last = None;
        while let Some(view) = game.next_view() {
            last = Some(view);
        }
        last.expect("the game plays on")
    }

    fn deciding(view: &PlayerView) -> Option<&DecisionView> {
        match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        }
    }

    fn find(d: &DecisionView, f: fn(&Candidate) -> bool) -> Option<usize> {
        d.candidates.iter().position(|c| f(&c.option))
    }

    /// A game against greedy in which the human plays a land when they can and otherwise
    /// passes, for `answers` answers. With `undo`, once they can from the fifth turn on, they
    /// take back their answers until they stand in the turn before, across the agent's, then
    /// answer as before, which plays the same game again.
    fn play(answers: usize, undo: bool) -> (ReplayFile, bool) {
        let start = StartGame {
            seed: 3,
            decks: [WU_CONTROL, WU_CONTROL]
                .map(|deck| deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()),
            human: Seat::P0,
            opponent: "greedy".to_owned(),
            stops: StopsSetting::SMART,
        };
        let mut game = Game::new(&CARDS, &start, SessionId(1)).unwrap();
        let mut view = to_human(&mut game);
        let (mut undone, mut answered) = (false, 0);
        while answered < answers {
            let Some(d) = deciding(&view) else { break };
            if undo && !undone && d.undo > 0 && view.turn.number >= 5 {
                let from = view.turn.number;
                while view.turn.number == from {
                    view = game.undo().unwrap();
                    answered -= 1;
                }
                undone = true;
                continue;
            }
            let pick = if d.kind == Kind::Priority {
                find(d, |c| matches!(c, Candidate::PlayLand { .. }))
                    .or_else(|| find(d, |c| matches!(c, Candidate::Pass)))
                    .unwrap_or(0)
            } else {
                0
            };
            let index = CandidateIndex(u16::try_from(pick).unwrap());
            game.choose(game.answer(d.id, index).unwrap());
            view = to_human(&mut game);
            answered += 1;
        }
        (game.replay().clone(), undone)
    }

    fn reviewed(file: &ReplayFile, name: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("beryl-review-{}-{name}.json", std::process::id()));
        file.save(&path).unwrap();
        let text = review(&path, Some("greedy"), None);
        std::fs::remove_file(&path).unwrap();
        text.unwrap()
    }

    /// A replay with an undo reviews as the game went: the game up to the undo, then again from
    /// the answer taken back, as a game played straight through shows it.
    #[test]
    fn review_goes_back_with_an_undo() {
        let (with_undo, undone) = play(40, true);
        assert!(undone && with_undo.log.contains(&Entry::Undo));
        let (straight, _) = play(40, false);
        let with_undo = reviewed(&with_undo, "undo");
        let straight = reviewed(&straight, "straight");
        let (before, _) = with_undo
            .split_once("(the human takes back an answer)")
            .expect("the review notes the undo");
        let (_, after) = with_undo
            .rsplit_once("(the human takes back an answer)")
            .unwrap();
        // The review heads the step gone back to again; from the next turn it reads as the
        // game played straight through.
        let after = &after[after.find("\n== Turn 4,").expect("back in turn 3")..];
        assert!(straight.starts_with(before), "before the undo:\n{before}");
        assert!(straight.ends_with(after), "after the undo:\n{after}");
        assert!(
            before.len() + after.len() > straight.len(),
            "the undo took back nothing"
        );
    }
}
