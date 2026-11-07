//! `beryl_cli review <replay.json> [--out FILE]`: an app game's transcript, with the agent's
//! search at each of its decisions. The replay file's log answers every decision; the agent,
//! rebuilt from the game's seed as the app built it, searches each of its own again, so its
//! root visits and mean rewards show why it chose as it did. A search that chooses other than
//! the log did is marked: the agent's code changed since the game, or the replay drifted.

use std::fmt::Write as _;
use std::path::Path;

use beryl_cards::decks::WU_CONTROL;
use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, Profile, Replay, Scratch, Seat, Small, State, Step};
use beryl_engine::{apply, next, observe, view};
use beryl_search::{Agent, Budget, agent};
use beryl_session::replay::Entry;
use beryl_session::{ReplayFile, with_stops};

use crate::game;
use crate::transcript::Transcript;

pub fn main(args: &[String]) -> Result<(), String> {
    let (path, out) = match args {
        [path] => (path, None),
        [path, flag, out] if flag == "--out" => (path, Some(out)),
        _ => return Err("usage: beryl_cli review <replay.json> [--out FILE]".to_owned()),
    };
    let text = review(Path::new(path))?;
    if let Some(out) = out {
        std::fs::write(out, text).map_err(|e| format!("{out}: {e}"))
    } else {
        print!("{text}");
        Ok(())
    }
}

fn review(path: &Path) -> Result<String, String> {
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
    let info = agent(&file.opponent).ok_or_else(|| format!("no agent {:?}", file.opponent))?;
    // As `beryl_session`'s `Game::new`: the agent draws from its own stream.
    let mut opponent = (info.new)(&t, SplitMix64::new(file.seed).next_u64(), Budget::DEFAULT);
    let mut s = State::new(&t, file.seed, with_stops(Profile::SMART, file.stops, human));
    let mut x = Scratch::recording();
    let names = if human == Seat::P0 {
        ["human", file.opponent.as_str()]
    } else {
        [file.opponent.as_str(), "human"]
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
