//! The long-think searcher: a seed and its settings reproduce a move on any number of
//! threads, the hidden searches decide from their Observation alone, a decision spends
//! about its budget of engine steps, and every leaf and method plays a game to its end.

mod common;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Decision, GameTable, Observation, Profile, Scratch, Seat, Slot, State, Zone, ZoneKey, observe,
};
use beryl_search::{Agent, Family, GreedyAgent, LongSearch, Search, Settings};
use common::{mirror, play};

/// Positions from a greedy mirror game with more than two candidates, `from_end` such
/// decisions before the game's last: late ones, for short rollouts.
fn positions(t: &GameTable, from_end: &[usize]) -> Vec<(State, Decision)> {
    let mut s = State::new(t, 4, Profile::SMART);
    let mut agents = [GreedyAgent::new(t), GreedyAgent::new(t)];
    let mut all = Vec::new();
    play(t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
        if d.len() > 2 {
            all.push((*s, *d));
        }
    });
    from_end.iter().map(|&n| all[all.len() - n]).collect()
}

fn settings(family: &str, work: u64, threads: u32) -> Settings {
    Settings {
        family: family.parse().unwrap(),
        work,
        threads,
    }
}

/// The search behind one decision from `s`.
fn search(t: &GameTable, settings: Settings, seed: u64, s: &State, d: &Decision) -> Search {
    let mut agent = LongSearch::new(t, settings, seed);
    let mut x = Scratch::new();
    agent.choose(observe(s, t, &mut x, d.player), d);
    agent.last_search().expect("a search").clone()
}

const FAMILIES: [&str; 6] = [
    "ismcts3/hidden/rollout/tuned",
    "pimc3/hidden/rollout/tuned",
    "pimc2/hands/cut1/estimate",
    "ismcts2/omniscient/eval/estimate",
    "ismcts3/hands/cut2/estimate",
    "pimc4/omniscient/rollout/estimate",
];

#[test]
fn a_seed_and_settings_reproduce_a_move_on_any_thread_count() {
    let t = mirror();
    for (i, (s, d)) in positions(&t, &[10, 5]).iter().enumerate() {
        for family in FAMILIES {
            let one = search(&t, settings(family, 3_000, 1), i as u64, s, d);
            assert_eq!(one.visits.len(), d.len());
            let three = search(&t, settings(family, 3_000, 3), i as u64, s, d);
            assert_eq!(one, three, "{family}, position {i}");
        }
    }
}

/// The hidden searches decide the same from the true state and from one with every card
/// they cannot see dealt anew; the omniscient one, which reads them, does not always.
#[test]
fn the_hidden_searches_decide_the_same_on_a_redeterminized_state() {
    let t = mirror();
    let mut rng = SplitMix64::new(11);
    let mut omniscient_differs = false;
    for (i, (s, d)) in positions(&t, &[23, 12, 5]).iter().enumerate() {
        let det = observe(s, &t, &mut Scratch::new(), d.player).determinize(&t, &mut rng);
        assert!(Slot::all().any(|slot| det.card(slot) != s.card(slot)));
        for family in ["ismcts2/hidden/rollout/tuned", "pimc2/hidden/cut1/estimate"] {
            let set = settings(family, 4_000, 1);
            assert_eq!(
                search(&t, set, 3, &det, d),
                search(&t, set, 3, s, d),
                "{family}, position {i}"
            );
        }
        let set = settings("ismcts2/omniscient/rollout/tuned", 4_000, 1);
        omniscient_differs |= search(&t, set, 3, &det, d) != search(&t, set, 3, s, d);
    }
    assert!(
        omniscient_differs,
        "the test can catch a search that cheats"
    );
}

/// Seeing the hands keeps every card in a hand as it is and deals the libraries anew.
#[test]
fn the_hands_sight_keeps_the_hands_and_deals_the_libraries() {
    let t = mirror();
    let (s, d) = positions(&t, &[10])[0];
    let obs = Observation::new(&s, d.player);
    let mut libraries_differ = false;
    for seed in 0..4 {
        let det = obs.determinize_libraries(&t, &mut SplitMix64::new(seed));
        for p in Seat::BOTH {
            for slot in s.zone(ZoneKey::Of(p, Zone::Hand)).iter() {
                assert_eq!(det.card(slot), s.card(slot));
            }
            libraries_differ |= s
                .library(p)
                .iter()
                .any(|&slot| det.card(slot) != s.card(slot));
        }
    }
    assert!(libraries_differ);
}

/// A decision spends its budget of engine steps, give or take the iteration that crosses it
/// on each tree, or less where the stop settles the move early; an evaluation costs far less
/// than a rollout, so the same budget buys many more iterations.
#[test]
fn a_decision_spends_about_its_budget() {
    let t = mirror();
    let (s, d) = positions(&t, &[30])[0];
    let work = 40_000;
    let rollout = search(
        &t,
        settings("ismcts4/hidden/rollout/tuned", work, 4),
        1,
        &s,
        &d,
    );
    assert!(
        rollout.work <= work + 4 * 2_000,
        "{} steps of {work}",
        rollout.work
    );
    let eval = search(
        &t,
        settings("ismcts4/hidden/eval/tuned", work, 4),
        1,
        &s,
        &d,
    );
    assert!(eval.work <= work + 4 * 100, "{} steps of {work}", eval.work);
    assert!(
        eval.rollouts > 5 * rollout.rollouts,
        "{} evaluations, {} rollouts",
        eval.rollouts,
        rollout.rollouts
    );
}

/// Every method, sight and leaf plays a mirror game to its end against the greedy agent.
#[test]
fn every_family_plays_a_game_to_its_end() {
    let t = mirror();
    for (i, family) in FAMILIES.iter().enumerate() {
        let mut s = State::new(&t, i as u64, Profile::SMART);
        let mut agents: [Box<dyn Agent>; 2] = [
            Box::new(LongSearch::new(&t, settings(family, 600, 2), i as u64)),
            Box::new(GreedyAgent::new(&t)),
        ];
        let mut decisions = 0;
        play(&t, &mut s, &mut Scratch::new(), &mut agents, |_, _| {
            decisions += 1;
        });
        assert!(decisions > 20, "{family}: {decisions} decisions");
    }
}

#[test]
fn families_name_themselves() {
    let f: Family = "ismcts18/hidden/rollout/estimate".parse().unwrap();
    assert_eq!(f.to_string(), "ismcts18/hidden/rollout/estimate");
}
