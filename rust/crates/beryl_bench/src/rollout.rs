//! The cube rollout benchmark (architecture §18): the workload search runs, measured in its
//! parts. A pinned corpus of mid-game WU Control mirror positions, cut from seeded games
//! between `expert` agents and between `greedy` agents, is rolled out to the end of the game
//! under the rollout policy and the search profile (smart stops), each position several
//! times from reseeded copies.
//!
//! The subjects, each over the whole corpus:
//!
//! - [`copy_states`]: one copy of a [`State`], as a rollout makes at its start;
//! - [`replay`]: the engine alone, `next` plus `apply`, replaying each rollout's recorded
//!   answers, so the policy costs nothing;
//! - [`decide`]: the rollout policy alone, on decisions sampled from the rollouts;
//! - [`roll`]: whole rollouts, a copy, the engine and the policy together.
//!
//! [`Summary`] gives the counts to divide by: rollouts, decisions and turns.

use std::hash::{Hash, Hasher};

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::{
    Answers, Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, State,
    Step, Stops, apply, next, observe,
};
use beryl_search::{Agent, ExpertAgent, GreedyAgent, Inline, RolloutPolicy, rollout};

/// The seed the corpus games are played from.
pub const SEED: u64 = 0x00C0_FFEE;

/// Games per agent the corpus is cut from.
pub const GAMES: u64 = 4;

/// The turns positions are cut at: each game gives its first decision on or after each of
/// them, while it lasts.
pub const CUTS: [u16; 3] = [5, 8, 11];

/// Rollouts per position, each from a copy reseeded so they diverge (§14).
pub const ROLLOUTS: u64 = 4;

/// Every how many decisions of a rollout the policy benchmark samples one.
pub const SAMPLE_EVERY: u64 = 16;

/// The mirror's table and the positions cut from its games.
pub struct Corpus {
    pub table: GameTable,
    pub positions: Vec<State>,
}

impl Corpus {
    /// Plays the seeded games and cuts their positions. Deterministic: the test
    /// `corpus_never_changes` pins it.
    pub fn new() -> Corpus {
        let table = GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).expect("the WU mirror");
        let mut positions = Vec::new();
        for expert in [true, false] {
            for game in 0..GAMES {
                let seed = SEED ^ (game << 1 | u64::from(expert));
                let mut agents: [Box<dyn Agent>; 2] = if expert {
                    [
                        Box::new(ExpertAgent::new(&table, seed)),
                        Box::new(ExpertAgent::new(&table, !seed)),
                    ]
                } else {
                    [
                        Box::new(GreedyAgent::new(&table)),
                        Box::new(GreedyAgent::new(&table)),
                    ]
                };
                cut(&table, seed, &mut agents, &mut positions);
            }
        }
        Corpus { table, positions }
    }

    /// The states the rollouts start from: each position [`ROLLOUTS`] times, reseeded, under
    /// the search profile.
    pub fn starts(&self) -> Vec<State> {
        let mut starts = Vec::new();
        for (i, position) in self.positions.iter().enumerate() {
            for r in 0..ROLLOUTS {
                let mut s = *position;
                s.reseed(SEED ^ ((i as u64) << 8 | r));
                s.set_profile(Profile {
                    stops: Stops::Smart,
                    breakpoint: None,
                    ..s.profile()
                });
                starts.push(s);
            }
        }
        starts
    }
}

impl Default for Corpus {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything the benchmarks measure, built before measuring: the corpus, the rollouts'
/// starts, their scripts and the sampled decisions.
pub struct Workload {
    pub table: GameTable,
    pub policy: RolloutPolicy,
    pub starts: Vec<State>,
    pub scripts: Vec<Script>,
    pub samples: Vec<(State, Decision)>,
    pub summary: Summary,
}

impl Workload {
    pub fn new() -> Workload {
        let corpus = Corpus::new();
        let starts = corpus.starts();
        let policy = RolloutPolicy::new(&corpus.table);
        let (scripts, samples, summary) = record(&corpus.table, &starts, &policy);
        Workload {
            table: corpus.table,
            policy,
            starts,
            scripts,
            samples,
            summary,
        }
    }

    /// The working memory a subject runs with: a scratch and a state to play on, allocated
    /// once so a measurement allocates nothing.
    pub fn memory(&self) -> (Scratch, Box<State>) {
        (Scratch::new(), Box::new(self.starts[0]))
    }
}

impl Default for Workload {
    fn default() -> Self {
        Self::new()
    }
}

/// Plays one seeded game under smart stops, keeping the state at its first decision on or
/// after each turn in [`CUTS`], until the last cut or the end of the game.
fn cut(t: &GameTable, seed: u64, agents: &mut [Box<dyn Agent>; 2], out: &mut Vec<State>) {
    let mut s = State::new(t, seed, Profile::SMART);
    let mut x = Scratch::new();
    let mut cuts = CUTS.iter().peekable();
    while let Some(&&turn) = cuts.peek() {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            return;
        };
        if s.turn().number >= turn {
            out.push(s);
            cuts.next();
        }
        let c = agents[d.player.index()].choose(observe(&s, t, &mut x, d.player), &d);
        apply(&mut s, t, &mut x, &mut Replay, c);
    }
}

/// A rollout's answers, the decisions' and the inline questions' alike, in the order the
/// engine asked them, for [`replay`].
pub struct Script {
    pub start: State,
    pub answers: Vec<Choice>,
}

/// What the rollouts did: the counts per-decision and per-rollout costs divide by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub rollouts: u64,
    /// Decisions `next` returned, each answered by the policy and applied.
    pub decisions: u64,
    /// Questions of resolving bodies the policy answered inline, within `next` and `apply`.
    pub inline: u64,
    /// Turns played from the starts to the ends.
    pub turns: u64,
}

impl Summary {
    pub fn decisions_per_rollout(&self) -> f64 {
        self.decisions as f64 / self.rollouts as f64
    }

    pub fn decisions_per_turn(&self) -> f64 {
        self.decisions as f64 / self.turns as f64
    }
}

/// The rollout policy inline, recording its answers.
struct Recording<'a> {
    inline: Inline<'a>,
    answers: Vec<Choice>,
}

impl Answers for Recording<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, obs: Observation<'_>, d: &Decision) -> Option<Choice> {
        let c = self.inline.answer(obs, d);
        self.answers.extend(c);
        c
    }
}

/// Plays each start out as [`rollout`] does, recording every answer, and samples the
/// decisions the policy answers, with the states they were asked in, every
/// [`SAMPLE_EVERY`].
pub fn record(
    t: &GameTable,
    starts: &[State],
    policy: &RolloutPolicy,
) -> (Vec<Script>, Vec<(State, Decision)>, Summary) {
    let mut x = Scratch::new();
    let (mut scripts, mut samples) = (Vec::new(), Vec::new());
    let mut summary = Summary::default();
    for start in starts {
        let before = summary.decisions;
        let mut s = *start;
        let mut a = Recording {
            inline: Inline { policy, t },
            answers: Vec::new(),
        };
        loop {
            match next(&mut s, t, &mut x, &mut a) {
                Step::Over(_) => break,
                Step::Decide(d) => {
                    if summary.decisions.is_multiple_of(SAMPLE_EVERY) {
                        samples.push((s, d));
                    }
                    summary.decisions += 1;
                    let c = policy.choose(t, Observation::new(&s, d.player), &d);
                    a.answers.push(c);
                    apply(&mut s, t, &mut x, &mut a, c);
                }
            }
        }
        summary.rollouts += 1;
        summary.inline += a.answers.len() as u64 - (summary.decisions - before);
        summary.turns += u64::from(s.turn().number - start.turn().number);
        scripts.push(Script {
            start: *start,
            answers: a.answers,
        });
    }
    (scripts, samples, summary)
}

/// Answers from a script, inline, in order.
struct Scripted<'a> {
    answers: std::slice::Iter<'a, Choice>,
}

impl Answers for Scripted<'_> {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, _obs: Observation<'_>, _d: &Decision) -> Option<Choice> {
        self.answers.next().copied()
    }
}

/// A number that depends on how a game ended, to fold into a benchmark's result.
fn score(outcome: Outcome, s: &State) -> u64 {
    let ended = match outcome {
        Outcome::Win(p) => p.index() as u64,
        Outcome::Draw => 2,
        Outcome::Stalled(_) => 3,
    };
    ended + u64::from(s.turn().number)
}

/// Copies each start into `into`, one `memcpy` each.
pub fn copy_states(starts: &[State], into: &mut State) -> u64 {
    let mut sum = 0;
    for start in starts {
        *into = *std::hint::black_box(start);
        sum += u64::from(std::hint::black_box(&*into).turn().number);
    }
    sum
}

/// Replays each script: the engine's `next` and `apply`, and nothing of the policy.
pub fn replay(t: &GameTable, scripts: &[Script], s: &mut State, x: &mut Scratch) -> u64 {
    let mut sum = 0;
    for script in scripts {
        *s = script.start;
        let mut a = Scripted {
            answers: script.answers.iter(),
        };
        let outcome = loop {
            match next(s, t, x, &mut a) {
                Step::Over(outcome) => break outcome,
                Step::Decide(_) => {
                    let c = *a.answers.next().expect("a scripted answer");
                    apply(s, t, x, &mut a, c);
                }
            }
        };
        sum += score(outcome, s);
    }
    sum
}

/// The policy's answer to each sampled decision.
pub fn decide(t: &GameTable, samples: &[(State, Decision)], policy: &RolloutPolicy) -> u64 {
    samples
        .iter()
        .map(|(s, d)| u64::from(policy.choose(t, Observation::new(s, d.player), d).0))
        .sum()
}

/// Rolls each start out under the rollout policy, from a copy, as search does.
pub fn roll(
    t: &GameTable,
    starts: &[State],
    policy: &RolloutPolicy,
    s: &mut State,
    x: &mut Scratch,
) -> u64 {
    let mut sum = 0;
    for start in starts {
        *s = *start;
        sum += score(rollout(s, t, x, policy), s);
    }
    sum
}

/// A hash of a value that never changes between runs or releases: 64-bit FNV-1a over what
/// its `Hash` impl feeds.
pub fn stable_hash(value: &impl Hash) -> u64 {
    struct Fnv(u64);
    impl Hasher for Fnv {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for &b in bytes {
                self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    value.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus is the benchmark's input: if it changed, every saved count and baseline
    /// would silently measure something else. A change to the engine, the cards or the
    /// agents that moves it updates this pin on purpose, and saved baselines are void.
    #[test]
    fn corpus_never_changes() {
        let c = Corpus::new();
        let turns: Vec<u16> = c.positions.iter().map(|s| s.turn().number).collect();
        assert_eq!(turns, CUTS.repeat(2 * GAMES as usize));
        assert_eq!(stable_hash(&c.positions), 10_336_105_418_688_469_987);
    }

    /// The engine benchmark replays the rollouts the whole-rollout benchmark plays: the
    /// same games to the same ends.
    #[test]
    fn replays_end_where_rollouts_end() {
        let c = Corpus::new();
        let starts = &c.starts()[..8];
        let policy = RolloutPolicy::new(&c.table);
        let (scripts, _, summary) = record(&c.table, starts, &policy);
        let mut x = Scratch::new();
        let mut s = starts[0];
        for (start, script) in starts.iter().zip(&scripts) {
            roll(
                &c.table,
                std::slice::from_ref(start),
                &policy,
                &mut s,
                &mut x,
            );
            let rolled = s;
            replay(&c.table, std::slice::from_ref(script), &mut s, &mut x);
            assert_eq!(stable_hash(&s), stable_hash(&rolled));
            assert!(s.outcome().is_some());
        }
        assert!(summary.decisions_per_rollout() > 20.0, "{summary:?}");
    }
}
