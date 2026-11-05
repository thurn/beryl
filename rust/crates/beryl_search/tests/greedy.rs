//! The greedy agent and the rollout policy behind it (architecture §19.1, decision D21), in
//! the WU Control mirror.

mod common;

use beryl_engine::{
    Answers, Choice, Decision, Observation, Outcome, Profile, Replay, Scratch, Seat, State, Step,
    apply, next,
};
use beryl_search::{Agent, GreedyAgent, Inline, RandomAgent, RolloutPolicy};
use common::{fingerprint, mirror, play};

/// Greedy against random, seats alternated. The CLI's 200-game tournament measures the
/// gate (at least 90%); this pins a smaller seeded sample so a weakened policy fails here.
#[test]
fn greedy_beats_random() {
    let t = mirror();
    let mut wins = 0;
    let games = 40;
    for seed in 0..games {
        let greedy = if seed % 2 == 0 { Seat::P0 } else { Seat::P1 };
        let mut agents: [Box<dyn Agent>; 2] = [
            Box::new(GreedyAgent::new(&t)),
            Box::new(RandomAgent::new(seed)),
        ];
        if greedy == Seat::P1 {
            agents.swap(0, 1);
        }
        let mut s = State::new(&t, seed, Profile::SMART);
        let outcome = play(&t, &mut s, &mut Scratch::new(), &mut agents, |s, _| {
            s.check_invariants(&t).unwrap();
        });
        if outcome == Outcome::Win(greedy) {
            wins += 1;
        }
        assert!(
            !matches!(outcome, Outcome::Stalled(_)),
            "seed {seed}: {outcome:?}"
        );
    }
    assert!(wins * 10 >= games * 9, "greedy won {wins} of {games}");
}

/// The inline answers, recorded in the order they were given.
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

/// A rollout answers mid-resolution questions inline, running each body once on the
/// committed state; replaying the same answers through pauses must reach the same game
/// (§7.3, §17.4).
#[test]
fn inline_answers_replay_to_the_same_game() {
    let t = mirror();
    let policy = RolloutPolicy::new(&t);
    let mut inlined = 0;
    for seed in 0..8 {
        let mut s = State::new(&t, seed, Profile::SMART);
        let mut x = Scratch::new();
        let mut asked = 0;
        let mut rec = Recording {
            inline: Inline {
                policy: &policy,
                t: &t,
            },
            answers: Vec::new(),
        };
        let outcome = loop {
            match next(&mut s, &t, &mut x, &mut rec) {
                Step::Over(outcome) => break outcome,
                Step::Decide(d) => {
                    asked += 1;
                    let c = policy.choose(&t, Observation::new(&s, d.player), &d);
                    rec.answers.push(c);
                    apply(&mut s, &t, &mut x, &mut rec, c);
                }
            }
        };
        let (final_hash, answers) = (fingerprint(&s), rec.answers);
        inlined += answers.len() - asked;

        let mut r = State::new(&t, seed, Profile::SMART);
        let mut x = Scratch::new();
        let mut answers = answers.into_iter();
        let replayed = loop {
            match next(&mut r, &t, &mut x, &mut Replay) {
                Step::Over(outcome) => break outcome,
                Step::Decide(_) => {
                    let c = answers.next().expect("as many answers as decisions");
                    apply(&mut r, &t, &mut x, &mut Replay, c);
                }
            }
        };
        assert_eq!(answers.next(), None, "seed {seed}: answers left over");
        assert_eq!(replayed, outcome, "seed {seed}");
        assert_eq!(fingerprint(&r), final_hash, "seed {seed}");
    }
    // Bodies really asked questions inline: the test has something to compare.
    assert!(inlined > 8, "{inlined} inline answers");
}
