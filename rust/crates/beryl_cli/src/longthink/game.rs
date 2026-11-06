//! One ladder game, resumable at any decision. Every decision's answer is logged as it is
//! made; a resumed game replays from its seed through the logged answers at engine speed and
//! asks its agents again only where the log ends.
//!
//! A search keeps no running state a replay could lose: its trees are seeded from the
//! agent's seed and the decision's index ([`Mcts::resumed`]). Its logged answers are taken
//! as they stand up to the point its last logged line says a rebuilt agent must start
//! from: the next decision, or, inside a chain of decisions reusing one search's trees, the
//! decision that planted them, re-searched from there to the same answers. Every other
//! agent is asked again for real, cheaply and deterministically, so its own state follows
//! the game. A replayed answer that differs from the logged one stops the game: the binary
//! no longer plays as the run did.

use std::time::Instant;

use beryl_engine::event::Event;
use beryl_engine::{
    Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, State, Step,
    apply, next, observe, view,
};
use beryl_search::{Agent, Budget, Mcts, Search, Sight, agent};
use serde::{Deserialize, Serialize};

use crate::game::seat_seeds;
use crate::transcript::Transcript;

/// An agent in a ladder game: a name in the registry, and a searching agent's budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrant {
    pub name: String,
    pub budget: Option<Budget>,
}

impl Entrant {
    /// How a transcript names it.
    pub fn label(&self) -> String {
        match self.budget {
            Some(b) => format!("{} {}", self.name, super::rung_name(b.iterations)),
            None => self.name.clone(),
        }
    }
}

/// One decision of a game, as its log keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub seat: u8,
    pub choice: u8,
    /// For a searching agent, the decision (its own index) a rebuilt agent must start from
    /// after this one ([`Mcts::resume_point`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<Searched>,
    /// Wall milliseconds spent deciding.
    pub ms: u64,
}

/// What a searched decision found and spent ([`Search`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Searched {
    /// The decision's index among the agent's own.
    pub decision: u32,
    pub chain: u32,
    pub rollouts: u32,
    pub inherited: u32,
    pub visits: Vec<u32>,
    pub rewards: Vec<f64>,
}

/// A seat's agent and, for a search, how far the log answers for it.
struct Seated {
    agent: Box<dyn Choose>,
    seat: u8,
    /// A search's decisions before `from` are taken from the log; `asked` counts its
    /// decisions so far.
    search: Option<Progress>,
}

struct Progress {
    from: u32,
    asked: u32,
}

impl Seated {
    /// Whether the agent's next decision is taken from the log, counting it if so.
    fn replays(&mut self) -> bool {
        match self.search.as_mut() {
            Some(p) if p.asked < p.from => {
                p.asked += 1;
                true
            }
            _ => false,
        }
    }

    /// Asks the agent decision `d`, and logs its answer.
    fn ask(&mut self, obs: Observation<'_>, d: &Decision) -> Line {
        let start = Instant::now();
        let c = self.agent.choose(obs, d);
        let ms = start.elapsed().as_millis() as u64;
        if let Some(p) = self.search.as_mut() {
            p.asked += 1;
        }
        let (search, resume) = match self.agent.searched() {
            Some((search, resume)) => (search.map(searched), Some(resume)),
            None => (None, None),
        };
        Line {
            seat: self.seat,
            choice: c.0,
            resume,
            search,
            ms,
        }
    }
}

/// Builds the agent `e` for a seat, from its seed, as it stands before its `from`th
/// decision. Tests substitute their own.
pub type Build<'a> = dyn Fn(&GameTable, &Entrant, u64, u32) -> Result<Box<dyn Choose>, String> + 'a;

/// An agent as a game asks it, with what a log needs of a search.
pub trait Choose {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice;
    /// The search behind the last answer, and where a rebuilt agent must start.
    fn searched(&self) -> Option<(Option<&Search>, u32)> {
        None
    }
}

impl Choose for Mcts {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        Agent::choose(self, obs, d)
    }

    fn searched(&self) -> Option<(Option<&Search>, u32)> {
        Some((self.last_search(), self.resume_point()))
    }
}

impl Choose for Box<dyn Agent + Send> {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        Agent::choose(self, obs, d)
    }
}

/// The agents as the registry builds them; a search as [`Mcts::resumed`] rebuilds it.
pub fn build(t: &GameTable, e: &Entrant, seed: u64, from: u32) -> Result<Box<dyn Choose>, String> {
    let sight = match e.name.as_str() {
        "mcts-hidden" => Some(Sight::Hidden),
        "mcts-omniscient" => Some(Sight::Omniscient),
        _ => None,
    };
    match (sight, e.budget) {
        (Some(sight), Some(budget)) => Ok(Box::new(Mcts::resumed(t, sight, budget, seed, from))),
        (Some(_), None) => Err(format!("{} needs a budget", e.name)),
        (None, _) => {
            let info = agent(&e.name).ok_or_else(|| format!("no agent is named {:?}", e.name))?;
            Ok(Box::new((info.new)(t, seed, Budget::DEFAULT)))
        }
    }
}

/// How a game went.
#[derive(Debug)]
pub struct Played {
    pub outcome: Outcome,
    pub turns: u16,
    /// Every decision's line, the logged ones and those made since.
    pub lines: Vec<Line>,
    pub transcript: String,
}

/// Why a game stopped short.
#[derive(Debug)]
pub enum Stop {
    /// The answer to decision `decision` (the game's, from 0) is not the logged one.
    Differs {
        decision: usize,
        logged: u8,
        now: u8,
    },
    /// Replay reached decision `until`, as asked.
    Reached(usize),
    Failed(String),
}

impl From<String> for Stop {
    fn from(e: String) -> Self {
        Stop::Failed(e)
    }
}

/// Where each seat's search starts asking rather than taking the log: where its last logged
/// line says a rebuilt agent must start, else at its first decision.
pub fn resume_points(logged: &[Line]) -> [u32; 2] {
    [0, 1].map(|seat| {
        logged
            .iter()
            .rev()
            .find(|l| l.seat == seat)
            .and_then(|l| l.resume)
            .unwrap_or(0)
    })
}

/// What a game needs besides its log.
pub struct Game<'a> {
    pub seed: u64,
    pub entrants: [&'a Entrant; 2],
    /// Each seat's search takes its decisions before this from the log.
    pub from: [u32; 2],
    /// Stop once this many decisions are answered.
    pub until: Option<usize>,
}

/// Plays `g` to its end through the `logged` answers, calling `asking` with the index, the
/// position and the decision before an agent is asked one not in the log, and `answered`
/// with each new decision's index, line and the transcript's label of its answer before it
/// is applied.
pub fn play(
    t: &GameTable,
    g: &Game<'_>,
    build: &Build<'_>,
    logged: &[Line],
    mut asking: impl FnMut(usize, &State, &Decision),
    mut answered: impl FnMut(usize, &Line, &str) -> Result<(), String>,
) -> Result<Played, Stop> {
    let seeds = seat_seeds(g.seed);
    let mut players = Vec::with_capacity(2);
    for ((e, seed), from) in g.entrants.iter().zip(seeds).zip(g.from) {
        players.push(Seated {
            agent: build(t, e, seed, from)?,
            seat: players.len() as u8,
            search: e.budget.map(|_| Progress { from, asked: 0 }),
        });
    }
    let mut s = State::new(t, g.seed, Profile::SMART);
    let mut x = Scratch::recording();
    let mut transcript = Transcript::new([
        g.entrants[0].label().as_str(),
        g.entrants[1].label().as_str(),
    ]);
    let mut lines = Vec::new();
    let outcome = loop {
        let i = lines.len();
        if g.until == Some(i) {
            return Err(Stop::Reached(i));
        }
        let d = match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => d,
        };
        let seat = d.player.index();
        let logged = logged.get(i);
        if let Some(l) = logged
            && (usize::from(l.seat) != seat || usize::from(l.choice) >= d.len())
        {
            return Err(Stop::Failed(format!(
                "decision {i}: the log answers seat {} with candidate {}, not one of seat \
                 {seat}'s {}",
                l.seat,
                l.choice,
                d.len()
            )));
        }
        let happened: Vec<Event> = x.events.drain().collect();
        let player = &mut players[seat];
        let (line, new) = match logged {
            Some(l) if player.replays() => (l.clone(), false),
            Some(l) => {
                let now = player.ask(observe(&s, t, &mut x, d.player), &d).choice;
                if now != l.choice {
                    return Err(Stop::Differs {
                        decision: i,
                        logged: l.choice,
                        now,
                    });
                }
                (l.clone(), false)
            }
            None => {
                asking(i, view(&s, t, &mut x), &d);
                (player.ask(observe(&s, t, &mut x, d.player), &d), true)
            }
        };
        let c = Choice(line.choice);
        let label = transcript.decision(t, view(&s, t, &mut x), &d, c, &happened);
        if new {
            answered(i, &line, &label)?;
        }
        apply(&mut s, t, &mut x, &mut Replay, c);
        lines.push(line);
    };
    if lines.len() < logged.len() {
        return Err(Stop::Failed(format!(
            "the game ended after {} decisions, but its log has {}",
            lines.len(),
            logged.len()
        )));
    }
    s.check_invariants(t)
        .map_err(|v| format!("invariant violated at the end: {v:?}"))?;
    let happened: Vec<Event> = x.events.drain().collect();
    transcript.end(t, &s, outcome, &happened);
    Ok(Played {
        outcome,
        turns: s.turn().number,
        lines,
        transcript: transcript.text().to_owned(),
    })
}

fn searched(s: &Search) -> Searched {
    Searched {
        decision: s.decision,
        chain: s.chain,
        rollouts: s.rollouts,
        inherited: s.inherited,
        visits: s.visits.clone(),
        rewards: s.rewards.clone(),
    }
}
