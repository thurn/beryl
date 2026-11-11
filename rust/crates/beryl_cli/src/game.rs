//! A headless game between two named agents in the WU Control mirror, seeded, optionally
//! checking the invariants at every decision and writing a transcript.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{GameTable, Outcome, Profile, Replay, Scratch, Small, State, Step};
use beryl_engine::{apply, next, observe, view};
use beryl_search::{Agent, Budget, Family, GameAgent, LongSearch, Settings, Widening, agent};

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

/// The seeds of game `seed`'s agents, each seat's from its own stream.
pub fn seat_seeds(seed: u64) -> [u64; 2] {
    let mut rng = SplitMix64::new(seed ^ 0xa9e1_7c3b_55d0_e4f1);
    [rng.next_u64(), rng.next_u64()]
}

/// The agents named for each seat, built for game `seed`, a searching one with `budget`.
pub fn agents(
    t: &GameTable,
    names: [&str; 2],
    seed: u64,
    budget: Budget,
) -> Result<[Box<dyn Agent + Send>; 2], String> {
    let seeds = seat_seeds(seed);
    let build = |seat: usize| {
        let name = names[seat];
        // Games may run one to a core, so a long-think entrant searches on one thread.
        if let Some(built) = entrant(t, name, seeds[seat], 1) {
            return built.map(|a| a as Box<dyn Agent + Send>);
        }
        let info = agent(name).ok_or_else(|| {
            let known: Vec<&str> = beryl_search::AGENTS.iter().map(|a| a.name).collect();
            format!("no agent is named {name:?} (agents: {})", known.join(", "))
        })?;
        Ok::<_, String>((info.new)(t, seeds[seat], budget))
    };
    Ok([build(0)?, build(1)?])
}

/// A long-think entrant, named as longthink names them (`family@budget`, as
/// `pimc18/omniscient/rollout/tuned/anchor+mid@3M`), for a game from `seed`, searching on
/// `threads`; none if `name` is not of that form.
pub fn entrant(
    t: &GameTable,
    name: &str,
    seed: u64,
    threads: u32,
) -> Option<Result<Box<dyn GameAgent + Send>, String>> {
    let (family, budget) = name.split_once('@')?;
    let built = family.parse::<Family>().and_then(|family| {
        let work = parse_work(budget).ok_or_else(|| format!("{name:?}: no budget {budget:?}"))?;
        let settings = Settings {
            family,
            work,
            threads,
        };
        Ok(
            Box::new(Widening::new(LongSearch::<Small>::new(t, settings, seed)))
                as Box<dyn GameAgent + Send>,
        )
    });
    Some(built)
}

/// A budget in engine steps, as longthink names it: 300k, 3M, 1G.
pub fn parse_work(s: &str) -> Option<u64> {
    let (digits, scale) = match s.as_bytes().last()? {
        b'k' => (&s[..s.len() - 1], 1_000),
        b'M' => (&s[..s.len() - 1], 1_000_000),
        b'G' => (&s[..s.len() - 1], 1_000_000_000),
        _ => (s, 1),
    };
    digits.parse::<u64>().ok()?.checked_mul(scale)
}

/// Plays game `seed` to its end, under the smart stops, searching agents with `budget`. With
/// `check`, the invariants are checked at every decision; they are always checked at the
/// end.
pub fn play(
    t: &GameTable,
    seed: u64,
    names: [&str; 2],
    budget: Budget,
    check: bool,
    mut transcript: Option<&mut Transcript>,
) -> Result<Played, String> {
    let mut agents = agents(t, names, seed, budget)?;
    let mut s = State::new(t, seed, Profile::SMART).map_err(|e| e.to_string())?;
    // A transcript tells what happened between decisions, from the engine's events.
    let mut x = if transcript.is_some() {
        Scratch::recording()
    } else {
        Scratch::new()
    };
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
                let happened: Vec<Event> = x.events.drain().collect();
                // A paused resolution is seen where its body stopped (§7.3).
                let c = agents[d.player.index()].choose(observe(&s, t, &mut x, d.player), &d);
                if let Some(tr) = transcript.as_deref_mut() {
                    tr.decision(t, view(&s, t, &mut x), &d, c, &happened);
                }
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    };
    violation(&s, "at the end")?;
    if let Some(tr) = transcript {
        let happened: Vec<Event> = x.events.drain().collect();
        tr.end(t, &s, outcome, &happened);
    }
    Ok(Played {
        outcome,
        turns: s.turn().number(),
        decisions,
    })
}
