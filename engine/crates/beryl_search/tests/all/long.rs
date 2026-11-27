//! The long-think searcher: a seed and its settings reproduce a move on any number of
//! threads, the hidden searches decide from their Observation alone, a decision spends
//! about its budget of engine steps, and every leaf and method plays a game to its end.

use crate::common::{mirror, play};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Choice, Decision, GameTable, Observation, Profile, Replay, Scratch, Seat, Slot, State, Step,
    Zone, ZoneKey, apply, next, observe,
};
use beryl_search::{
    Agent, AgentDecision, Budget, DEEP_SEARCHER, Family, GreedyAgent, Knowledge, LongSearch, Mcts,
    Options, RolloutPolicy, Search, Settings, Sight,
};

/// Positions from a greedy mirror game with more than two candidates, `from_end` such
/// decisions before the game's last: late ones, for short rollouts.
fn positions(t: &GameTable, from_end: &[usize]) -> Vec<(State, Decision)> {
    let mut s = State::new(t, 4, Profile::SMART).unwrap();
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
    agent.choose(observe(s, t, &mut x, d.decider), d, AgentDecision(0));
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
        let det = observe(s, &t, &mut Scratch::new(), d.decider).determinize(&t, &mut rng);
        assert!(Slot::all().any(|slot| det.card_behind(slot) != s.card_behind(slot)));
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
    let obs = Observation::new(&s, d.decider);
    let mut libraries_differ = false;
    for seed in 0..4 {
        let det = obs.determinize_libraries(&t, &mut SplitMix64::new(seed));
        for p in Seat::BOTH {
            let hand = s.zone(ZoneKey::Of(p, Zone::Hand));
            assert!(hand.all(|slot| det.card_behind(slot) == s.card_behind(slot)));
            libraries_differ |= s
                .library(p)
                .iter()
                .any(|&slot| det.card_behind(slot) != s.card_behind(slot));
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
        let mut s = State::new(&t, i as u64, Profile::SMART).unwrap();
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
fn expert_anchored_search_replays_a_game_from_its_seed() {
    let t = mirror();
    let settings = settings("pimc2/hidden/cut1/tuned/anchor+expert", 600, 1);
    let mut s = State::new(&t, 4, Profile::SMART).unwrap();
    let mut x = Scratch::new();
    let mut first = LongSearch::new(&t, settings, 5);
    let mut replay = LongSearch::resumed(&t, settings, 5, AgentDecision(0));
    let mut opponent = GreedyAgent::new(&t);
    let mut checked = 0;
    let mut own = AgentDecision(0);
    while let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) {
        let obs = observe(&s, &t, &mut x, d.decider);
        let choice = if d.player == Seat::BOTH[0] {
            let at = own.take();
            let choice = first.choose(obs, &d, at);
            assert_eq!(choice, replay.choose(obs, &d, at));
            assert_eq!(first.last_search(), replay.last_search());
            assert_eq!(first.resume_point(), Some(AgentDecision(0)));
            checked += usize::from(first.last_search().is_some_and(|s| s.anchor.is_some()));
            choice
        } else {
            opponent.choose(obs, &d, AgentDecision(0))
        };
        apply(&mut s, &t, &mut x, &mut Replay, choice);
    }
    assert!(checked > 0, "the game must exercise the anchor");
}

/// A family's name parses and prints back unchanged, the champion's with the speed bonus too,
/// and the champion itself scores with the full turn discount.
#[test]
fn families_name_themselves() {
    let f: Family = "ismcts18/hidden/rollout/estimate".parse().unwrap();
    assert_eq!(f.to_string(), "ismcts18/hidden/rollout/estimate");
    const { assert!(!DEEP_SEARCHER.options.bonus) };
    let name = format!("{DEEP_SEARCHER}+bonus");
    let f: Family = name.parse().unwrap();
    assert!(f.options.bonus);
    assert_eq!(f.to_string(), name);
    assert_eq!(
        Family {
            options: Options {
                bonus: false,
                ..f.options
            },
            ..f
        },
        DEEP_SEARCHER
    );
}

/// `fitroll` and `fitroot` each print back as given, and each requires `tuned` knowledge
/// and the other's absence, with an error that names the reason.
#[test]
fn fitroll_and_fitroot_name_themselves_and_require_tuned_alone() {
    for name in [
        "pimc2/hidden/rollout/tuned/anchor+fitroll",
        "pimc2/hidden/rollout/tuned/fitroot",
    ] {
        let f: Family = name.parse().unwrap();
        assert_eq!(f.to_string(), name);
    }
    for (name, reason) in [
        (
            "pimc2/hidden/rollout/tuned/fitroll+fitroot",
            "fitted knowledge",
        ),
        ("pimc2/hidden/rollout/fitted/fitroll", "require tuned"),
        ("pimc2/hidden/rollout/estimate/fitroot", "require tuned"),
    ] {
        let error = name.parse::<Family>().unwrap_err();
        assert!(error.contains(reason), "{name}: {error}");
    }
}

/// At decisions where tuned greedy and fitted greedy answer differently, the default the
/// anchor checks the search against is tuned's under `fitroll`, which fits only the
/// simulations, and fitted's under `fitroot`, which fits only the root.
#[test]
fn fitroll_and_fitroot_take_the_default_from_their_root_values() {
    let t = mirror();
    let tuned = RolloutPolicy::with(&t, Knowledge::Tuned);
    let fitted = RolloutPolicy::with(&t, Knowledge::Fitted);
    let mut x = Scratch::new();
    let mut differ = Vec::new();
    for seed in 0..4 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let mut agents = [GreedyAgent::new(&t), GreedyAgent::new(&t)];
        play(&t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
            let obs = observe(s, &t, &mut x, d.decider);
            let own_on_top = obs
                .stack()
                .last()
                .is_some_and(|e| e.controller() == d.player);
            if differ.len() < 12 && d.len() > 1 && !own_on_top {
                let answers = [tuned.choose(&t, obs, d), fitted.choose(&t, obs, d)];
                if answers[0] != answers[1] {
                    differ.push((*s, *d, answers));
                }
            }
        });
    }
    assert!(
        !differ.is_empty(),
        "no decision where tuned and fitted differ"
    );
    // `first` holds the default against any other choice of the search's, so every
    // decision where the trees prefer something else records its check.
    for (option, side) in [("fitroll", 0), ("fitroot", 1)] {
        let family = format!("pimc2/hidden/rollout/tuned/anchor+paired+first+{option}");
        let mut checked = 0;
        for (i, (s, d, answers)) in differ.iter().enumerate() {
            let found = search(&t, settings(&family, 20_000, 1), i as u64, s, d);
            if let Some(anchor) = found.anchor {
                assert_eq!(anchor.greedy, answers[side], "{option} at decision {i}");
                checked += 1;
            }
        }
        assert!(checked > 0, "{option}: no decision recorded its default");
    }
}

/// The opponent's Stock Up took five seconds of thinking, half of it ordering the three cards
/// it put on the bottom of its library, one search a card. Neither search spends anything on
/// such a pick: its rollouts almost never reach those cards. The positions are real ones
/// from greedy mirror games, where Stock Up asks the order of the rest.
#[test]
fn neither_search_thinks_about_the_order_of_cards_for_the_library_bottom() {
    let t = mirror();
    let mut orders = Vec::new();
    for seed in 0..16 {
        let mut s = State::new(&t, seed, Profile::SMART).unwrap();
        let mut agents = [GreedyAgent::new(&t), GreedyAgent::new(&t)];
        play(&t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
            if d.len() > 1 && d.prompt.orders_library_bottom() {
                orders.push((*s, *d));
            }
        });
    }
    assert!(!orders.is_empty(), "no game asked for a bottom order");
    let mut x = Scratch::new();
    for (s, d) in &orders {
        let obs = observe(s, &t, &mut x, d.decider);
        let mut long = LongSearch::new(&t, settings("pimc2/hidden/rollout/tuned", 3_000, 1), 1);
        assert_eq!(long.choose(obs, d, AgentDecision(0)), Choice(0));
        assert_eq!(long.last_search(), None);
        let budget = Budget {
            iterations: 6,
            trees: 1,
            parallel: false,
        };
        let mut mcts = Mcts::new(&t, Sight::Hidden, budget, 1);
        assert_eq!(mcts.choose(obs, d, AgentDecision(0)), Choice(0));
        assert_eq!(mcts.last_search(), None);
    }
}

/// `cede` stops a paired, anchored search once the default answer's lead cannot be overturned
/// before the challenger check: over late positions of a greedy game, it plays the same moves
/// as the search without it, never spends more, and spends less at some decision.
#[test]
fn cede_plays_the_same_moves_for_less_work() {
    let t = mirror();
    let plain = "pimc4/hidden/rollout/tuned/anchor+guard+paired";
    let ceding = format!("{plain}+cede");
    let run = |family: &str, s: &State, d: &Decision| {
        let mut agent = LongSearch::new(&t, settings(family, 200_000, 1), 7);
        let mut x = Scratch::new();
        let choice = agent.choose(observe(s, &t, &mut x, d.decider), d, AgentDecision(0));
        (choice, agent.last_search().map_or(0, |search| search.work))
    };
    let mut saved = 0;
    for (s, d) in positions(&t, &(2..=10).collect::<Vec<_>>()) {
        let (move_plain, work_plain) = run(plain, &s, &d);
        let (move_ceding, work_ceding) = run(&ceding, &s, &d);
        assert_eq!(move_ceding, move_plain);
        assert!(work_ceding <= work_plain, "{work_ceding} > {work_plain}");
        saved += work_plain - work_ceding;
    }
    assert!(saved > 0, "cede never stopped early");
}

/// A family with `cede` prints back as given, and `cede` requires `paired` and the absence
/// of `means`.
#[test]
fn cede_names_itself_and_requires_paired() {
    let name = "pimc2/hidden/rollout/tuned/anchor+paired+cede";
    assert_eq!(name.parse::<Family>().unwrap().to_string(), name);
    for bad in [
        "pimc2/hidden/rollout/tuned/anchor+cede",
        "pimc2/hidden/rollout/tuned/anchor+means+paired+cede",
    ] {
        assert!(bad.parse::<Family>().is_err(), "{bad} parsed");
    }
}
