//! A headless game between two named agents in the WU Control mirror, seeded, optionally
//! checking the invariants at every decision and writing a transcript.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{GameTable, Observation, Outcome, Profile, Replay, Scratch, State, Step};
use beryl_engine::{apply, next, view};
use beryl_search::{Agent, agent};

use crate::transcript::Transcript;

/// The table every CLI game is played on: the Milestone 1 mirror.
pub fn mirror() -> GameTable {
    GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).expect("the WU Control mirror's table")
}

/// How a game went.
#[derive(Clone, Copy, Debug)]
pub struct Played {
    pub outcome: Outcome,
    /// The turn the game ended on.
    pub turns: u16,
    pub decisions: u32,
}

/// The agents named for each seat, built for game `seed`, each from its own stream.
pub fn agents(
    t: &GameTable,
    names: [&str; 2],
    seed: u64,
) -> Result<[Box<dyn Agent + Send>; 2], String> {
    let mut rng = SplitMix64::new(seed ^ 0xa9e1_7c3b_55d0_e4f1);
    let mut build = |name: &str| {
        let info = agent(name).ok_or_else(|| {
            let known: Vec<&str> = beryl_search::AGENTS.iter().map(|a| a.name).collect();
            format!("no agent is named {name:?} (agents: {})", known.join(", "))
        })?;
        Ok::<_, String>((info.new)(t, rng.next_u64()))
    };
    Ok([build(names[0])?, build(names[1])?])
}

/// Plays game `seed` to its end, under the smart stops. With `check`, the invariants are
/// checked at every decision; they are always checked at the end.
pub fn play(
    t: &GameTable,
    seed: u64,
    names: [&str; 2],
    check: bool,
    mut transcript: Option<&mut Transcript>,
) -> Result<Played, String> {
    let mut agents = agents(t, names, seed)?;
    let mut s = State::new(t, seed, Profile::SMART);
    let mut x = Scratch::new();
    let mut decisions = 0;
    let violation = |s: &State, at: &str| {
        s.check_invariants(t)
            .map_err(|v| format!("seed {seed}: invariant violated {at}: {v:?}"))
    };
    let outcome = loop {
        match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => {
                decisions += 1;
                if check {
                    violation(&s, &format!("at decision {decisions}"))?;
                }
                // A paused resolution is seen where its body stopped (§7.3).
                let shown = view(&s, t, &mut x);
                let c = agents[d.player.index()].choose(Observation::new(shown, d.player), &d);
                if let Some(tr) = transcript.as_deref_mut() {
                    tr.decision(t, shown, &d, c);
                }
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    };
    violation(&s, "at the end")?;
    if let Some(tr) = transcript {
        tr.end(&s, outcome);
    }
    Ok(Played {
        outcome,
        turns: s.turn().number,
        decisions,
    })
}
