use beryl_cards::{CARDS, names};
use beryl_engine::{Profile, next};
use beryl_scenario::{P0, P1, Permanent, Scenario, position};

use super::*;
use crate::{Agent, Settings};

const FAMILY: &str = "turn72/hidden/rollout/tuned/anchor+guard";
const ISLAND: Permanent = Permanent::new(names::ISLAND);
const PLAINS: Permanent = Permanent::new(names::PLAINS);
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
    let obs = Observation::new(&state, d.player);
    let swapped = obs.determinize(&t, &mut SplitMix64::new(3));
    assert!(
        beryl_engine::Slot::all().any(|slot| swapped.card_behind(slot) != state.card_behind(slot))
    );
    let mut first = agent(&t, 300_000, FAMILY);
    let mut second = agent(&t, 300_000, FAMILY);
    let choice = first.choose(obs, &d, AgentDecision(0));
    assert_eq!(
        choice,
        second.choose(Observation::new(&swapped, d.player), &d, AgentDecision(0))
    );
    assert_eq!(first.last_search(), second.last_search());
    let report = first.last_search().unwrap();
    assert!(report.turn.unwrap().completed > 0);
    assert!(report.rollouts > 0);
    assert!(
        report
            .visits
            .iter()
            .all(|&n| n == 0 || n == u32::from(SCORING_WORLDS))
    );
    assert!(report.work <= 300_000);
}

#[test]
fn a_small_budget_falls_back_and_each_priority_uses_its_own_position() {
    let (t, mut state) = position(&CARDS, &OPENING).unwrap();
    let mut scratch = Scratch::new();
    let mut player = agent(&t, 16, FAMILY);
    for index in 0..2 {
        let Step::Decide(d) = next(&mut state, &t, &mut scratch, &mut Replay) else {
            panic!("priority in main");
        };
        assert_eq!(d.player, P0);
        let obs = Observation::new(&state, d.player);
        let expected = player.policy.choose(&t, obs, &d);
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
            apply_decided(&mut state, &t, &mut scratch, &mut Replay, &d, choice);
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
    let obs = Observation::new(&state, d.player);
    let mut turn = agent(&t, 20_000, FAMILY);
    let mut pimc = agent(&t, 20_000, "pimc72/hidden/rollout/tuned/anchor+guard");
    assert_eq!(
        turn.choose(obs, &d, AgentDecision(0)),
        pimc.choose(obs, &d, AgentDecision(0))
    );
    assert_eq!(turn.last_search(), pimc.last_search());
    assert!(turn.last_search().unwrap().turn.is_none());
}

#[test]
fn the_beam_merges_equal_states_but_keeps_distinct_engine_positions() {
    let (_, state) = position(&CARDS, &OPENING).unwrap();
    let mut beam = Beam::default();
    let mut report = TurnReport::default();
    let mut original = Line::new(state);
    original.regret = 3;
    beam.push(original, &mut report);
    beam.push(Line::new(state), &mut report);
    let mut other = state;
    other.reseed(9);
    beam.push(Line::new(other), &mut report);
    assert_eq!(report.merged, 1);
    assert_eq!(beam.lines.len(), 2);
    assert_eq!(beam.lines[0].1.regret, 0);
    assert_eq!(*beam.lines[0].1.state, state);
    assert_ne!(other, state);
}

#[test]
fn response_reserve_distinguishes_equal_amounts_of_mana() {
    const RESPONSE: Scenario = Scenario::new("keep two blue")
        .at(5, P1, TurnStep::Upkeep)
        .hand(P0, &[names::MANA_DRAIN]);
    for (permanents, keeps_counter) in [(&[ISLAND; 2], true), (&[ISLAND, PLAINS], false)] {
        let (t, mut state) = position(&CARDS, &RESPONSE.permanents(P0, permanents)).unwrap();
        let mut scratch = Scratch::new();
        assert!(
            next_within(
                &mut state,
                &t,
                &mut scratch,
                &mut Replay,
                (5, TurnStep::Upkeep),
                0
            )
            .is_none()
        );
        let options = payable_responses(&state, &t, P0).unwrap();
        assert_eq!(
            options.iter().any(|o| matches!(o, Opt::Cast { .. })),
            keeps_counter
        );
    }
}

#[test]
fn nested_combat_spends_the_shared_allowance_and_can_stop() {
    const FIGHT: Scenario = Scenario::new("attack into a blocker")
        .at(4, P0, TurnStep::BeginningOfCombat)
        .permanents(P0, &[Permanent::new(names::SNAPCASTER_MAGE)])
        .permanents(P1, &[Permanent::new(names::SNAPCASTER_MAGE)])
        .library(P0, LIBRARY)
        .library(P1, LIBRARY);
    let (t, mut state) = position(&CARDS, &FIGHT).unwrap();
    state.set_profile(Profile::SMART);
    let mut scratch = Scratch::unchecked();
    let d = loop {
        let Step::Decide(d) = next(&mut state, &t, &mut scratch, &mut Replay) else {
            panic!("attacker declaration");
        };
        if d.kind == DecisionKind::Attacker {
            break d;
        }
        assert_eq!(d.kind, DecisionKind::Priority);
        apply_decided(&mut state, &t, &mut scratch, &mut Replay, &d, Choice(0));
    };
    assert_eq!(d.kind, DecisionKind::Attacker);
    let policy = RolloutPolicy::new(&t);
    let plan = |limit, scratch: &mut Scratch| {
        combat::Sim::<_, combat::General>::new(
            &t,
            &policy,
            scratch,
            P0,
            &mut combat::Plan::default(),
        )
        .choose_within(&state, &d, limit)
    };
    let mut full = Scratch::unchecked();
    assert!(plan(20_000, &mut full).is_some());
    assert!(
        full.counts.state_copies > 3,
        "combat simulated the defender's blocks"
    );
    let limit = full.counts.decision_work() / 2;
    let mut limited = Scratch::unchecked();
    assert_eq!(plan(limit, &mut limited), None);
    assert!(limited.counts.decision_work() <= limit);
    assert!(limited.counts.state_copies > 0);
}
