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
use beryl_search::{Agent, Budget, GameAgent, Search, agent};
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
    let mut stops = file.stops;
    // The position an undo goes back to: the turn and the state at the human's first stop in a
    // turn of theirs, as `Game::undo` keeps it.
    let mut turn_start: Option<(u16, State)> = None;
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(outcome) => {
                let happened: Vec<Event> = x.events.drain().collect();
                transcript.end(&t, &s, outcome, &happened);
                break;
            }
            Step::Decide(d) => {
                // As `beryl_session`'s `Game::replay`: a stops change takes effect at the
                // decision open when it was made, and a halt is the session's, not the engine's.
                let before = s;
                let mut halted = false;
                let entry = loop {
                    match log.next() {
                        Some(&Entry::Stops(changed)) => {
                            stops = changed;
                            s.set_profile(with_stops(s.profile(), stops, human));
                        }
                        Some(Entry::Halt) => halted = true,
                        Some(Entry::Concede) => {
                            transcript.note("(the human conceded)");
                            break None;
                        }
                        entry => break entry.copied(),
                    }
                };
                // The session keeps the turn's start where it first stops for the human in
                // their turn: at a halt, which only decisionless play separates from this
                // decision, or at a decision of theirs it did not answer for them.
                let turn = before.turn();
                if turn.active == human
                    && turn.number > 0
                    && turn_start.is_none_or(|(n, _)| n != turn.number)
                    && (halted
                        || d.player == human
                            && matches!(entry, Some(Entry::Choice(_) | Entry::Undo)))
                {
                    turn_start = Some((turn.number, before));
                }
                let c = match entry {
                    Some(Entry::Choice(c) | Entry::Auto(c)) => c,
                    Some(Entry::Undo) => {
                        let Some((number, start)) = turn_start else {
                            return Err("an undo with nothing to go back to".to_owned());
                        };
                        // Stops changed since stay changed, as `Game::undo` keeps them.
                        s = start;
                        s.set_profile(with_stops(s.profile(), stops, human));
                        x.events.drain().for_each(drop);
                        transcript
                            .note(&format!("(the human undoes to the start of turn {number})"));
                        continue;
                    }
                    _ => {
                        transcript.note("(the record ends here)");
                        break;
                    }
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
                    transcript.note(&search_line(&search, chose, c));
                }
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
    }
    Ok(transcript.text().to_owned())
}

/// A search's root candidates, visits and mean reward each, its check against greedy's
/// answer, and whether it chose other than the game's `logged` answer.
fn search_line(search: &Search, chose: Choice, logged: Choice) -> String {
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
    if chose != logged {
        let _ = write!(line, "  REPLAY CHOSE [{}]", chose.0);
    }
    line
}

#[cfg(test)]
mod tests {
    use beryl_cards::CARDS;
    use beryl_session::protocol::{Candidate, DecisionView, Kind};
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
    /// passes, for `answers` answers. With `undo`, once past the start of a turn of theirs from
    /// the fifth on they undo to it, then answer as before, which plays the same game again.
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
            if undo && !undone && d.undo.is_some() && view.turn.number >= 5 {
                view = game.undo().unwrap();
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
        let text = review(&path, Some("greedy"));
        std::fs::remove_file(&path).unwrap();
        text.unwrap()
    }

    /// A replay with an undo reviews as the game went: the turn up to the undo, then that turn
    /// again from its start, as a game played straight through shows it.
    #[test]
    fn review_goes_back_with_an_undo() {
        let (with_undo, undone) = play(40, true);
        assert!(undone && with_undo.log.contains(&Entry::Undo));
        let (straight, _) = play(40, false);
        let with_undo = reviewed(&with_undo, "undo");
        let straight = reviewed(&straight, "straight");
        let (before, after) = with_undo
            .split_once("(the human undoes to the start of turn")
            .expect("the review notes the undo");
        let (_, after) = after.split_once('\n').unwrap();
        assert!(straight.starts_with(before), "before the undo:\n{before}");
        assert!(straight.ends_with(after), "after the undo:\n{after}");
        assert!(
            before.len() + after.len() > straight.len(),
            "the undo took back nothing"
        );
    }
}
