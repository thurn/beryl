//! Transactional replay (architecture §7.3, §17.4): a game whose bodies are answered inline
//! and the same game replayed through `Replay` reach the same state at every decision.
//!
//! A replacement effect's choice is answered the same two ways, in a resolution and in the
//! draw step, where the end of the step pauses ([`Pending::StepActions`]).

use beryl_card::{CardDef, Face, ManaCost, Replacement, Types};

use super::cast::*;
use super::{DUAL, LAND, fingerprint};
use crate::card::Ability;
use crate::rng::SplitMix64;
use crate::state::Pending;
use crate::{
    Answers, CardTable, Choice, Decision, Decklist, GameTable, Observation, Outcome, Payment,
    Profile, Replay, Scratch, State, Step, apply, next,
};

/// A 1/1 with dredge 2, to die or be discarded into a graveyard and offer to replace draws
/// from there.
static DREDGER: CardDef = CardDef {
    name: "Test Dredger",
    tokens: &[],
    faces: &[Face {
        cost: ManaCost::new(1, [0; 6]),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::replacing(&Replacement::dredge(2))],
        ..Face::EMPTY
    }],
};

/// The test cards and the dredger.
static DREDGE_CARDS: CardTable = CardTable::new(&[
    &LAND, &ISLAND, &DUAL, &CRYPT, &TREASURE, &STRIP, &HQ, &BEAR, &REMOVAL, &TITHE, &LIES, &PONDER,
    &FLASHER, &DIVER, &TRAINER, &OVERLORD, &STUPOR, &RAVELER, &EMPEROR, &FLASHBACK, &BINDER,
    &LOOTER, &STINGER, &SNAPBACK, &BOUNCE, &ELF, &SAGE, &RUMMAGE, &DREDGER,
]);

/// Lands, cards that discard and kill, and four dredgers.
const DREDGE_DECK: &Decklist = &[
    (10, "Test Land"),
    (6, "Test Island"),
    (2, "Test Dual"),
    (4, "Test Dredger"),
    (2, "Test Bear"),
    (2, "Test Removal"),
    (2, "Test Lies"),
    (2, "Test Ponder"),
    (1, "Test Looter"),
    (1, "Test Rummage"),
    (1, "Test Stinger"),
    (1, "Test Sage"),
];

/// The cards and the deck both players play.
type Decks = (&'static CardTable, &'static Decklist);

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

/// A random game of `decks` with inline answers, under `profile`.
fn inline_game(decks: Decks, seed: u64, profile: Profile) -> (Vec<Logged>, Outcome) {
    let t = GameTable::new(decks.0, [decks.1, decks.1]).unwrap();
    let mut s = State::new(&t, seed, profile).unwrap();
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

/// How many questions a replay paused at: in a resolution, and as a step ended.
#[derive(Debug, Default)]
struct Paused {
    resolving: usize,
    step_actions: usize,
}

/// Replays an inline game's answers through `Replay`, checking the state hash at each of
/// its decisions. Returns how many questions paused.
fn replay(
    decks: Decks,
    seed: u64,
    profile: Profile,
    logged: &[Logged],
    outcome: Outcome,
) -> Paused {
    let t = GameTable::new(decks.0, [decks.1, decks.1]).unwrap();
    let mut s = State::new(&t, seed, profile).unwrap();
    let mut x = Scratch::new();
    let mut log = logged.iter();
    let mut paused = Paused::default();
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(o) => {
                assert_eq!(o, outcome, "seed {seed}");
                assert_eq!(log.next(), None, "seed {seed}: answers left over");
                return paused;
            }
            Step::Decide(_) => {
                let pause = match s.pending() {
                    Pending::Resolving(_) => Some(&mut paused.resolving),
                    Pending::StepActions(_) => Some(&mut paused.step_actions),
                    _ => None,
                };
                let choice = if let Some(count) = pause {
                    *count += 1;
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
    let decks = (&CAST_CARDS, EVERYTHING);
    for seed in 0..3 {
        for profile in [Profile::EXACT, Profile::SMART, greedy, manual] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            finished += 1;
            paused += replay(decks, seed, profile, &logged, outcome).resolving;
        }
    }
    assert_eq!(finished, 12);
    assert!(
        paused > 20,
        "only {paused} paused questions: the test exercises too little"
    );
}

/// Dredge's choice, answered inline as a rollout would and replayed through `Replay`, reaches
/// the same state: in the draw step, where the end of the step pauses, and in a resolution
/// that draws.
#[test]
fn replacement_choices_replay_to_the_same_state_in_a_step_and_a_resolution() {
    let decks = (&DREDGE_CARDS, DREDGE_DECK);
    let mut paused = Paused::default();
    for seed in 0..4 {
        for profile in [Profile::EXACT, Profile::SMART] {
            let (logged, outcome) = inline_game(decks, seed, profile);
            assert!(
                !matches!(outcome, Outcome::Stalled(_)),
                "seed {seed}: {outcome:?}"
            );
            let p = replay(decks, seed, profile, &logged, outcome);
            paused.resolving += p.resolving;
            paused.step_actions += p.step_actions;
        }
    }
    assert!(
        paused.step_actions >= 3 && paused.resolving >= 3,
        "{paused:?}: the test exercises too little"
    );
}
