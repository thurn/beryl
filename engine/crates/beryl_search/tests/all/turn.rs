//! The turn search (`turn72`): a decision's answer does not depend on the cards hidden from its
//! player, it falls back to the rollout policy when its budget is too small to search, and on
//! the opponent's turn it searches as the plain hidden-information search does.

use beryl_cards::{CARDS, names};
use beryl_engine::GameTable;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{Observation, Opt, Replay, Scratch, Step, TurnStep, apply, next};
use beryl_scenario::{P0, P1, Permanent, Scenario, position};
use beryl_search::{Agent, AgentDecision, Family, LongSearch, Settings};

const FAMILY: &str = "turn72/hidden/rollout/tuned/anchor+guard";
const ISLAND: Permanent = Permanent::new(names::ISLAND);
const LIBRARY: &[&str] = &[
    names::ISLAND,
    names::PLAINS,
    names::MANA_DRAIN,
    names::SNAPCASTER_MAGE,
    names::ISLAND,
    names::PLAINS,
    names::MANA_DRAIN,
    names::SNAPCASTER_MAGE,
];
const OPENING: Scenario = Scenario::new("own main")
    .at(4, P0, TurnStep::PrecombatMain)
    .hand(P0, &[names::ISLAND, names::SNAPCASTER_MAGE])
    .permanents(P0, &[ISLAND; 2])
    .library(P0, LIBRARY)
    .library(P1, LIBRARY);

fn agent(t: &GameTable, work: u64, family: &str) -> LongSearch {
    LongSearch::new(
        t,
        Settings {
            family: family.parse().unwrap(),
            work,
            threads: 1,
        },
        11,
    )
}

#[test]
fn turn_choice_is_independent_of_hidden_cards() {
    let (t, mut state) = position(&CARDS, &OPENING).unwrap();
    let Step::Decide(d) = next(&mut state, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("priority in main");
    };
    let obs = Observation::new(&state, d.decider);
    let swapped = obs.determinize(&t, &mut SplitMix64::new(3));
    assert!(
        beryl_engine::Slot::all().any(|slot| swapped.card_behind(slot) != state.card_behind(slot))
    );
    let mut first = agent(&t, 300_000, FAMILY);
    let mut second = agent(&t, 300_000, FAMILY);
    let choice = first.choose(obs, &d, AgentDecision(0));
    assert_eq!(
        choice,
        second.choose(Observation::new(&swapped, d.decider), &d, AgentDecision(0))
    );
    assert_eq!(first.last_search(), second.last_search());
    let report = first.last_search().unwrap();
    assert!(report.turn.unwrap().completed > 0);
    assert!(report.rollouts > 0);
    // Every candidate that was scored was scored in the same worlds as the others.
    let scored: Vec<u32> = report.visits.iter().copied().filter(|&n| n > 0).collect();
    assert!(scored.iter().all(|&n| n == scored[0]), "{scored:?}");
    assert!(report.work <= 300_000);
}

#[test]
fn budgeted_turn_search_scores_more_complete_worlds() {
    let (t, mut state) = position(&CARDS, &OPENING).unwrap();
    let Step::Decide(d) = next(&mut state, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("priority in main");
    };
    let obs = Observation::new(&state, d.decider);
    let work = 300_000;
    let mut bounded = agent(&t, work, FAMILY);
    let mut budgeted = agent(&t, work, "turnbudget72/hidden/rollout/tuned/anchor+guard");
    bounded.choose(obs, &d, AgentDecision(0));
    budgeted.choose(obs, &d, AgentDecision(0));

    let bounded = bounded.last_search().unwrap();
    let budgeted = budgeted.last_search().unwrap();
    let most_worlds = bounded.visits.iter().copied().max().unwrap();
    let mut scored = budgeted.visits.iter().copied().filter(|&n| n > 0);
    let worlds = scored.next().unwrap();
    assert!(worlds > most_worlds);
    assert!(scored.all(|n| n == worlds));
    assert!(!budgeted.turn.unwrap().defaulted);
    assert!(budgeted.work <= work);
}

#[test]
fn a_small_budget_falls_back_and_each_priority_uses_its_own_position() {
    let (t, mut state) = position(&CARDS, &OPENING).unwrap();
    let mut scratch = Scratch::new();
    let mut player = agent(&t, 16, FAMILY);
    let policy = FAMILY.parse::<Family>().unwrap().rollout_policy(&t);
    for index in 0..2 {
        let Step::Decide(d) = next(&mut state, &t, &mut scratch, &mut Replay) else {
            panic!("priority in main");
        };
        assert_eq!(d.player, P0);
        let obs = Observation::new(&state, d.decider);
        let expected = policy.choose(&t, obs, &d);
        let choice = player.choose(obs, &d, AgentDecision(index));
        assert_eq!(choice, expected);
        let report = player.last_search().unwrap();
        assert_eq!(report.chain, AgentDecision(index));
        assert!(report.turn.unwrap().defaulted);
        assert!(report.work <= 16);
        if index == 0 {
            assert!(matches!(
                d.candidates.option(choice),
                Some(Opt::PlayLand(_))
            ));
            apply(&mut state, &t, &mut scratch, &mut Replay, choice);
        }
    }
}

#[test]
fn opponent_turn_priority_uses_the_same_search_as_pimc() {
    let (t, mut state) = position(&CARDS, &OPENING.at(4, P1, TurnStep::End)).unwrap();
    state.give_priority(P0);
    let Step::Decide(d) = next(&mut state, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("priority on the opponent's turn");
    };
    assert_eq!(d.player, P0);
    let obs = Observation::new(&state, d.decider);
    let mut turn = agent(&t, 20_000, FAMILY);
    let mut pimc = agent(&t, 20_000, "pimc72/hidden/rollout/tuned/anchor+guard");
    assert_eq!(
        turn.choose(obs, &d, AgentDecision(0)),
        pimc.choose(obs, &d, AgentDecision(0))
    );
    assert_eq!(turn.last_search(), pimc.last_search());
    assert!(turn.last_search().unwrap().turn.is_none());
}
