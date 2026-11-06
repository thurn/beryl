//! Random games over the cards implemented so far (architecture §17.4): fuzzing with the
//! invariants after every decision, determinism, information independence and the perf
//! counts. `BERYL_FUZZ_GAMES=<n>` plays more games; `just fuzz <n>` does that in an optimized
//! build that keeps the debug assertions.

mod common;

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    CardIx, Choice, Counts, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch,
    Seat, Slot, StallReason, State, Step, Zone, apply, next,
};
use beryl_search::{Agent, GreedyAgent, RandomAgent};
use common::{agents, fingerprint, game, games, mirror, play};

#[test]
fn random_games_keep_the_invariants() {
    let n = games(40);
    let mut stalls = 0;
    for seed in 0..n {
        let (t, mut s) = game(seed);
        let mut decisions = 0;
        let outcome = play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                decisions += 1;
                assert!(!d.is_empty(), "seed {seed}: {d:?}");
                if let Err(v) = s.check_invariants(&t) {
                    panic!("seed {seed}, decision {decisions}: {v:?}");
                }
            },
        );
        if let Err(v) = s.check_invariants(&t) {
            panic!("seed {seed}, at the end: {v:?}");
        }
        match outcome {
            Outcome::Stalled(StallReason::Capacity) => panic!("seed {seed}: a capacity ran out"),
            Outcome::Stalled(_) => stalls += 1,
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    // The stall-rate target (architecture §1): below 0.5% of random games.
    assert!(stalls * 200 < n, "{stalls} of {n} games stalled");
}

/// Plays a game with random agents, recording every choice and the state hash at every
/// decision.
fn record(seed: u64) -> (Vec<Choice>, Vec<u64>) {
    let (t, mut s) = game(seed);
    let mut x = Scratch::new();
    let mut agents = agents(seed);
    let (mut choices, mut hashes) = (Vec::new(), Vec::new());
    loop {
        match next(&mut s, &t, &mut x, &mut Replay) {
            Step::Over(_) => return (choices, hashes),
            Step::Decide(d) => {
                hashes.push(fingerprint(&s));
                let c = agents[d.player.index()].choose(Observation::new(&s, d.player), &d);
                choices.push(c);
                apply(&mut s, &t, &mut x, &mut Replay, c);
            }
        }
    }
}

#[test]
fn a_seed_and_its_choices_reproduce_every_state_hash() {
    for seed in 0..4 {
        let (choices, hashes) = record(seed);
        assert!(hashes.len() > 40, "seed {seed}: a short game");
        let (t, mut s) = game(seed);
        let mut x = Scratch::new();
        let mut replayed = Vec::new();
        let mut choices = choices.into_iter();
        while let Step::Decide(_) = next(&mut s, &t, &mut x, &mut Replay) {
            replayed.push(fingerprint(&s));
            apply(&mut s, &t, &mut x, &mut Replay, choices.next().unwrap());
        }
        assert_eq!(replayed, hashes, "seed {seed}");
        assert_ne!(record(seed + 4).1, hashes, "seeds {seed} and {}", seed + 4);
    }
}

/// Decides by hashing everything its Observation shows, so a view that leaks anything hidden
/// decides differently on two states that differ only in what is hidden.
struct Observer;

impl Agent for Observer {
    fn choose(&mut self, obs: Observation<'_>, d: &Decision) -> Choice {
        let mut h = DefaultHasher::new();
        for slot in Slot::all() {
            (obs.knows(slot), obs.card(slot), obs.object(slot)).hash(&mut h);
        }
        for p in Seat::BOTH {
            obs.player(p).hash(&mut h);
            obs.library(p).hash(&mut h);
            obs.graveyard(p).hash(&mut h);
            for zone in [
                Zone::Library,
                Zone::Hand,
                Zone::Battlefield,
                Zone::Graveyard,
                Zone::Exile,
            ] {
                obs.zone(p, zone).hash(&mut h);
            }
        }
        (obs.viewer(), obs.turn(), obs.battlefield()).hash(&mut h);
        Choice((h.finish() % d.len() as u64) as u8)
    }
}

#[test]
fn agents_decide_the_same_on_a_redeterminized_state() {
    let mut rng = SplitMix64::new(7);
    let (mut decisions, mut redealt) = (0, 0);
    for seed in 0..games(10) {
        let (t, mut s) = game(seed);
        play(
            &t,
            &mut s,
            &mut Scratch::new(),
            &mut agents(seed),
            |s, d| {
                let viewer = d.player;
                let mut det = Observation::new(s, viewer).determinize(&t, &mut rng);
                det.check_invariants(&t).unwrap();
                for slot in Slot::all() {
                    if s.knows(viewer, slot) {
                        assert_eq!(det.object(slot), s.object(slot), "slot {slot}");
                    }
                }
                let ours = Observer.choose(Observation::new(s, viewer), d);
                let theirs = Observer.choose(Observation::new(&det, viewer), d);
                assert_eq!(
                    ours, theirs,
                    "seed {seed}: the view of a determinized state differs"
                );
                // The engine offers the viewer the same decision there.
                match next(&mut det, &t, &mut Scratch::new(), &mut Replay) {
                    Step::Decide(again) => assert_eq!(again, *d, "seed {seed}"),
                    Step::Over(o) => panic!("seed {seed}: the determinized game ended: {o:?}"),
                }
                decisions += 1;
                if Slot::all().any(|slot| det.card(slot) != s.card(slot)) {
                    redealt += 1;
                }
            },
        );
    }
    // Hidden cards really were dealt anew: the test has something to catch.
    assert!(redealt * 2 > decisions, "{redealt} of {decisions}");
}

#[test]
fn determinization_keeps_the_size_of_every_hidden_group() {
    let (t, mut s) = game(3);
    play(&t, &mut s, &mut Scratch::new(), &mut agents(3), |s, d| {
        let det = Observation::new(s, d.player).determinize(&t, &mut SplitMix64::new(1));
        for p in Seat::BOTH {
            let cards = |s: &State| {
                let mut v: Vec<CardIx> = Slot::all()
                    .filter(|&slot| {
                        s.object(slot).owner == p && s.object(slot).zone != Zone::Nowhere
                    })
                    .map(|slot| s.card(slot))
                    .collect();
                v.sort_unstable();
                v
            };
            assert_eq!(cards(&det), cards(s), "player {p} has the same deck");
        }
    });
}

/// The agents of a seeded WU Control mirror game: both random or both greedy.
fn mirror_agents(t: &GameTable, greedy: bool, seed: u64) -> [Box<dyn Agent>; 2] {
    if greedy {
        [Box::new(GreedyAgent::new(t)), Box::new(GreedyAgent::new(t))]
    } else {
        [
            Box::new(RandomAgent::new(seed * 2)),
            Box::new(RandomAgent::new(seed * 2 + 1)),
        ]
    }
}

/// The perf counts of one seeded WU Control mirror game under smart stops, and its outcome.
fn mirror_game(t: &GameTable, greedy: bool, seed: u64) -> (Counts, Outcome) {
    let mut s = State::new(t, seed, Profile::SMART);
    let mut x = Scratch::new();
    let outcome = play(
        t,
        &mut s,
        &mut x,
        &mut mirror_agents(t, greedy, seed),
        |_, _| {},
    );
    (x.counts, outcome)
}

/// The perf counts of seeded games (architecture §17.4), each with the value it is pinned at,
/// within a tolerance band of 5%: a change that moves one out of its band either fixes a
/// regression or updates the pin on purpose.
#[test]
fn perf_counts_stay_in_their_bands() {
    // (greedy, seed): decisions, silent, state-based checks, state copies, layer recomputes,
    // solver runs, events.
    let pinned: [(bool, u64, [u64; 7]); 6] = [
        (false, 0, [165, 526, 640, 44, 81, 51, 603]),
        (false, 1, [153, 475, 574, 76, 83, 51, 605]),
        (false, 2, [215, 559, 679, 80, 81, 57, 761]),
        (true, 0, [156, 541, 648, 98, 93, 68, 696]),
        (true, 1, [252, 564, 751, 100, 89, 76, 804]),
        (true, 2, [202, 662, 782, 124, 100, 82, 870]),
    ];
    let t = mirror();
    let mut outside = Vec::new();
    for (greedy, seed, pinned) in pinned {
        let (c, _) = mirror_game(&t, greedy, seed);
        let actual = [
            c.decisions,
            c.silent,
            c.state_based_checks,
            c.state_copies,
            c.layer_recomputes,
            c.solver_runs,
            c.events,
        ];
        let names = [
            "decisions",
            "silent",
            "state_based_checks",
            "state_copies",
            "layer_recomputes",
            "solver_runs",
            "events",
        ];
        for ((name, actual), pinned) in names.into_iter().zip(actual).zip(pinned) {
            if actual.abs_diff(pinned) * 100 > pinned * 5 {
                outside.push((greedy, seed, name, actual, pinned));
            }
        }
    }
    assert!(
        outside.is_empty(),
        "outside their bands (greedy, seed, name, actual, pinned): {outside:?}"
    );
}

/// The stall-rate target (architecture §1): under 0.5% of seeded WU Control mirror games,
/// random and greedy, stall.
#[test]
fn mirror_games_rarely_stall() {
    let t = mirror();
    let n = games(25);
    let mut stalls = Vec::new();
    for greedy in [false, true] {
        for seed in 0..n {
            if let (_, Outcome::Stalled(reason)) = mirror_game(&t, greedy, seed) {
                stalls.push((greedy, seed, reason));
            }
        }
    }
    assert!(
        stalls.len() * 200 < 2 * n as usize,
        "{} of {} games stalled: {stalls:?}",
        stalls.len(),
        2 * n
    );
}
