//! Transactional replay (architecture §7.3, §17.4): a game whose bodies are answered inline
//! and the same game replayed through `Replay` reach the same state at every decision.

use super::cast::{CAST_CARDS, EVERYTHING};
use super::fingerprint;
use crate::rng::SplitMix64;
use crate::state::Pending;
use crate::{
    Answers, Choice, Decision, GameTable, Observation, Outcome, Payment, Profile, Replay, Scratch,
    State, Step, apply, next,
};

/// Answers every question at once, at random, as a rollout policy would, and logs its answers.
struct Inline {
    rng: SplitMix64,
    log: Vec<u16>,
}

impl Answers for Inline {
    fn inline(&self) -> bool {
        true
    }

    fn answer(&mut self, _obs: Observation<'_>, d: &Decision) -> Option<Choice> {
        let c = self.rng.below(d.len() as u64) as u16;
        self.log.push(c);
        Some(Choice(c))
    }
}

/// What the inline game did, in order.
#[derive(Debug, PartialEq, Eq)]
enum Logged {
    /// A decision `next` returned: the state's hash, and the answer.
    Decision { hash: u64, choice: u16 },
    /// A question a body asked, answered inline.
    Inline(u16),
}

/// A random game with inline answers, under `profile`.
fn inline_game(seed: u64, profile: Profile) -> (Vec<Logged>, Outcome) {
    let t = GameTable::new(&CAST_CARDS, [EVERYTHING, EVERYTHING]).unwrap();
    let mut s = State::new(&t, seed, profile);
    let mut x = Scratch::new();
    let mut answers = Inline {
        rng: SplitMix64::new(seed ^ 0xA5A5),
        log: Vec::new(),
    };
    let mut agent = SplitMix64::new(seed);
    let mut logged = Vec::new();
    let drain = |answers: &mut Inline, logged: &mut Vec<Logged>| {
        logged.extend(answers.log.drain(..).map(Logged::Inline));
    };
    let outcome = loop {
        let step = next(&mut s, &t, &mut x, &mut answers);
        drain(&mut answers, &mut logged);
        match step {
            Step::Over(outcome) => break outcome,
            Step::Decide(d) => {
                let choice = agent.below(d.len() as u64) as u16;
                logged.push(Logged::Decision {
                    hash: fingerprint(&s),
                    choice,
                });
                apply(&mut s, &t, &mut x, &mut answers, Choice(choice));
                drain(&mut answers, &mut logged);
            }
        }
    };
    (logged, outcome)
}

/// Replays an inline game's answers through `Replay`, checking the state hash at each of
/// its decisions. Returns how many questions paused.
fn replay(seed: u64, profile: Profile, logged: &[Logged], outcome: Outcome) -> usize {
    let t = GameTable::new(&CAST_CARDS, [EVERYTHING, EVERYTHING]).unwrap();
    let mut s = State::new(&t, seed, profile);
    let mut x = Scratch::new();
    let mut log = logged.iter();
    let mut paused = 0;
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(o) => {
                assert_eq!(o, outcome, "seed {seed}");
                assert_eq!(log.next(), None, "seed {seed}: answers left over");
                return paused;
            }
            Step::Decide(_) => {
                let choice = if matches!(s.pending(), Pending::Resolving(_)) {
                    paused += 1;
                    match log.next() {
                        Some(&Logged::Inline(c)) => c,
                        other => panic!("seed {seed}: paused where the inline game had {other:?}"),
                    }
                } else {
                    match log.next() {
                        Some(&Logged::Decision { hash, choice }) => {
                            assert_eq!(fingerprint(&s), hash, "seed {seed}: the states diverged");
                            choice
                        }
                        other => {
                            panic!("seed {seed}: a decision where the inline game had {other:?}")
                        }
                    }
                };
                apply(&mut s, &t, &mut x, &mut Replay, Choice(choice));
            }
        }
    }
}

#[test]
fn inline_and_replayed_answers_reach_the_same_state_at_every_decision() {
    let mut paused = 0;
    let mut finished = 0;
    let greedy = Profile {
        payment: Payment::Greedy,
        ..Profile::SMART
    };
    let manual = Profile {
        payment: Payment::Manual,
        ..Profile::EXACT
    };
    for seed in 0..8 {
        for profile in [Profile::EXACT, Profile::SMART, greedy, manual] {
            let (logged, outcome) = inline_game(seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            finished += 1;
            paused += replay(seed, profile, &logged, outcome);
        }
    }
    assert_eq!(finished, 32);
    assert!(
        paused > 20,
        "only {paused} paused questions: the test exercises too little"
    );
}
