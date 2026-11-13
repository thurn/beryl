//! One arena game, resumable at any decision. Every decision's answer is logged as it is
//! made; a resumed game replays from its seed through the logged answers at engine speed and
//! asks its agents again only where the log ends.
//!
//! A search's trees are seeded from the agent's seed and the decision's index
//! ([`LongSearch::resumed`]). Its logged answers are taken
//! as they stand up to the point its last logged line says a rebuilt agent must start
//! from: the next decision, or, inside a chain of decisions reusing one search's trees, the
//! decision that planted them, re-searched from there to the same answers. The expert yardstick
//! starts at the first decision to rebuild its combat plan. Every other
//! agent is asked again for real, cheaply and deterministically, so its own state follows
//! the game. A replayed answer that differs from the logged one stops the game: the binary
//! no longer plays as the run did.

use std::fmt;
use std::time::Instant;

use beryl_engine::event::Event;
use beryl_engine::{
    Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, State, Step,
    apply, next, observe, view,
};
use beryl_search::{Agent, AgentDecision, Search, Settings};
use serde::{Deserialize, Serialize};

use crate::game::seat_seeds;
use crate::transcript::Transcript;

use super::AnchorRecord;

/// A decision's index in the game, from 0, counting both players' decisions: not its index
/// among the deciding agent's own ([`AgentDecision`]).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GameDecision(pub usize);

impl fmt::Display for GameDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// An agent in an arena game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entrant {
    /// The long-think search, with its settings.
    Search(Settings),
    /// An agent of the registry that does not search, by name.
    Fixed(String),
}

impl Entrant {
    /// Its name in the arena: `ismcts18/hidden/rollout/tuned@3M`, or the fixed agent's.
    pub fn label(&self) -> String {
        match self {
            Entrant::Search(s) => format!("{}@{}", s.family, super::rung_name(s.work)),
            Entrant::Fixed(name) => name.clone(),
        }
    }

    /// A search's budget, in engine steps a decision.
    pub fn budget(&self) -> Option<u64> {
        match self {
            Entrant::Search(s) => Some(s.work),
            Entrant::Fixed(_) => None,
        }
    }
}

/// One decision of a game, as its log keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub seat: u8,
    pub choice: u16,
    /// For a searching agent, the decision of its own a rebuilt agent must start from after
    /// this one ([`Agent::resume_point`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<AgentDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<Searched>,
    /// Wall milliseconds spent deciding.
    pub ms: u64,
}

/// What a searched decision found and spent ([`Search`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Searched {
    pub decision: AgentDecision,
    /// The decision whose search planted the trees this one grew.
    pub chain: AgentDecision,
    pub rollouts: u32,
    pub inherited: u32,
    /// The engine steps it spent.
    #[serde(default)]
    pub work: u64,
    pub visits: Vec<u32>,
    pub rewards: Vec<f64>,
    /// Each candidate's mean averaged over the trees ([`Search::means`]); empty in a log
    /// written before it was kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub means: Vec<Option<f64>>,
    /// An anchored family's check of its choice against greedy's answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<AnchorRecord>,
}

/// How much of a search's play a game takes from its log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    /// All of it: the search is never asked.
    Log,
    /// Its decisions before this one; it is asked from there.
    At(AgentDecision),
}

/// A seat's agent and, for a search, how far the log answers for it.
struct Seated {
    agent: Box<dyn Agent>,
    /// The agent's next decision among its own, asked or taken from the log.
    own: AgentDecision,
    seat: u8,
    search: Option<Progress>,
}

struct Progress {
    from: Resume,
    /// The search's next decision.
    asked: AgentDecision,
}

impl Seated {
    /// Whether the agent's next decision is taken from the log, counting it if so.
    fn replays(&mut self) -> bool {
        let Some(p) = self.search.as_mut() else {
            return false;
        };
        let replays = match p.from {
            Resume::Log => true,
            Resume::At(from) => p.asked < from,
        };
        if replays {
            p.asked = p.asked.next();
            self.own = self.own.next();
        }
        replays
    }

    /// Asks the agent decision `d`, and logs its answer.
    fn ask(&mut self, obs: Observation<'_>, d: &Decision) -> Line {
        let start = Instant::now();
        let c = self.agent.choose(obs, d, self.own.take());
        let ms = start.elapsed().as_millis() as u64;
        if let Some(p) = self.search.as_mut() {
            p.asked = p.asked.next();
        }
        Line {
            seat: self.seat,
            choice: c.0,
            resume: self.agent.resume_point(),
            search: self.agent.last_search().map(searched),
            ms,
        }
    }
}

/// Builds the agent `e` for a seat, from its seed, as it stands before its own decision
/// `from`. Tests substitute their own.
pub type Build<'a> =
    dyn Fn(&GameTable, &Entrant, u64, AgentDecision) -> Result<Box<dyn Agent>, String> + 'a;

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
    /// The answer to decision `decision` is not the logged one.
    Differs {
        decision: GameDecision,
        logged: u16,
        now: u16,
    },
    /// The game stopped before decision `i`, as asked: at `until`, or where `asking` said
    /// to. It resumes from its log.
    Reached(GameDecision),
    Failed(String),
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Differs {
                decision,
                logged,
                now,
            } => write!(f, "decision {decision}: answered {now}, logged {logged}"),
            Stop::Reached(decision) => write!(f, "stopped before decision {decision}"),
            Stop::Failed(e) => f.write_str(e),
        }
    }
}

impl From<String> for Stop {
    fn from(e: String) -> Self {
        Stop::Failed(e)
    }
}

/// What a game needs besides its log.
pub struct Game<'a> {
    pub seed: u64,
    pub entrants: [&'a Entrant; 2],
    /// How much of each seat's search the log answers for.
    pub from: [Resume; 2],
    /// Stop before asking this decision.
    pub until: Option<GameDecision>,
    /// The state the game starts from, where not the shuffle of `seed`: a ladder position
    /// ([`crate::positions`]).
    pub start: Option<State>,
}

/// Each seat's agent for game `g`, built as its log says a rebuilt agent must start.
fn seat(t: &GameTable, g: &Game<'_>, build: &Build<'_>) -> Result<Vec<Seated>, Stop> {
    let seeds = seat_seeds(g.seed);
    let mut players = Vec::with_capacity(2);
    for ((e, seed), from) in g.entrants.iter().zip(seeds).zip(g.from) {
        // A search the log answers for entirely is never asked: where it starts is moot.
        let start = match from {
            Resume::Log => AgentDecision(0),
            Resume::At(d) => d,
        };
        players.push(Seated {
            agent: build(t, e, seed, start)?,
            own: AgentDecision(0),
            seat: players.len() as u8,
            search: matches!(e, Entrant::Search(_)).then_some(Progress {
                from,
                asked: AgentDecision(0),
            }),
        });
    }
    Ok(players)
}

/// Plays `g` to its end through the `logged` answers, calling `asking` with the index, the
/// position and the decision before an agent is asked one not in the log, which stops the
/// game there if it returns an error, and `answered` with each new decision's index, line
/// and the transcript's label of its answer before it is applied.
pub fn play(
    t: &GameTable,
    g: &Game<'_>,
    build: &Build<'_>,
    logged: &[Line],
    mut asking: impl FnMut(GameDecision, &State, &Decision) -> Result<(), Stop>,
    mut answered: impl FnMut(GameDecision, &Line, &str) -> Result<(), String>,
) -> Result<Played, Stop> {
    let mut players = seat(t, g, build)?;
    let mut s = match g.start {
        Some(start) => start,
        None => State::new(t, g.seed, Profile::SMART).map_err(|e| e.to_string())?,
    };
    let mut x = Scratch::recording();
    let mut transcript = Transcript::new([
        g.entrants[0].label().as_str(),
        g.entrants[1].label().as_str(),
    ]);
    let mut lines = Vec::new();
    let outcome = loop {
        let i = GameDecision(lines.len());
        if g.until == Some(i) {
            return Err(Stop::Reached(i));
        }
        let d = match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => d,
        };
        let seat = d.player.index();
        let logged = logged.get(i.0);
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
            None if player
                .search
                .as_ref()
                .is_some_and(|p| p.from == Resume::Log) =>
            {
                return Err(Stop::Failed(format!(
                    "decision {i}: the log ends before seat {seat}'s search's play"
                )));
            }
            None => {
                asking(i, view(&s, t, &mut x), &d)?;
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
        turns: s.turn().number(),
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
        work: s.work,
        visits: s.visits.clone(),
        rewards: s.rewards.clone(),
        means: s.means.clone(),
        anchor: s.anchor.map(|a| AnchorRecord {
            greedy: a.greedy.0,
            search: a.search.0,
            lead: a.lead,
            error: a.error,
            trees: a.trees,
        }),
    }
}
