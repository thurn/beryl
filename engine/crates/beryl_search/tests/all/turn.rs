//! The turn search (`turn72`): a decision's answer does not depend on the cards hidden from its
//! player, it falls back to the rollout policy when its budget is too small to search, and on
//! the opponent's turn it searches as the plain hidden-information search does.

use beryl_cards::{CARDS, names};
use beryl_engine::Choice;
use beryl_engine::GameTable;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{Observation, Opt, Replay, Scratch, Step, TurnStep, apply, next};
use beryl_scenario::{P0, P1, Permanent, Scenario, position};
use beryl_search::{Agent, AgentDecision, Family, LongSearch, Search, Settings};

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

const RACE: &str = "turnbudget72/hidden/rollout/tuned/anchor+guard+race";
const PLAINS: Permanent = Permanent::new(names::PLAINS);
/// P0's Gurmag Angler can attack P1, at 5 life, for lethal damage this turn. Casting Damn
/// from P0's hand would destroy it, and casting Humility would shrink it to 1/1.
const LETHAL: Scenario = Scenario::new("lethal attack")
    .at(4, P0, TurnStep::PrecombatMain)
    .life(P1, 5)
    .permanents(P0, &[PLAINS; 4])
    .battlefield(P0, &[names::GURMAG_ANGLER])
    .library(P0, LIBRARY)
    .library(P1, LIBRARY);

/// Searches the first decision of `scenario` with `family` and returns the choice, the
/// default greedy's answer, and the search's report.
fn search(scenario: &Scenario, family: &str) -> (Choice, Choice, Search) {
    let (t, mut state) = position(&CARDS, scenario).unwrap();
    let Step::Decide(d) = next(&mut state, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("priority in main");
    };
    let obs = Observation::new(&state, d.decider);
    let default = family
        .parse::<Family>()
        .unwrap()
        .rollout_policy(&t)
        .choose(&t, obs, &d);
    let mut searcher = agent(&t, 300_000, family);
    let choice = searcher.choose(obs, &d, AgentDecision(0));
    (choice, default, searcher.last_search().unwrap().clone())
}

/// With three root moves (pass, play Plains, cast Damn on P0's own attacker), `race` drops
/// Damn after the first checks and spends the work on more worlds for the other two. Without
/// it, every root is scored in every world and nothing is dropped. Both choose alike, and
/// a second search from the same seed answers the same.
#[test]
fn race_drops_a_losing_root_and_scores_more_worlds() {
    let lethal = LETHAL.hand(P0, &[names::DAMN, names::PLAINS]);
    let (plain_choice, _, plain) =
        search(&lethal, "turnbudget72/hidden/rollout/tuned/anchor+guard");
    let (choice, default, raced) = search(&lethal, RACE);
    assert_eq!(raced.visits.len(), 3);
    assert_eq!(choice, plain_choice);
    assert_eq!(search(&lethal, RACE), (choice, default, raced.clone()));

    let plain_report = plain.turn.unwrap();
    assert_eq!(plain_report.dropped, 0);
    assert!(
        plain
            .visits
            .iter()
            .all(|&n| u64::from(n) == plain_report.worlds)
    );

    let report = raced.turn.unwrap();
    assert!(report.dropped >= 1, "{report:?}");
    assert!(
        report.worlds > plain_report.worlds,
        "{report:?} {plain_report:?}"
    );
    assert_eq!(
        u64::from(raced.visits[usize::from(default.0)]),
        report.worlds
    );
    assert!(raced.work <= 300_000);
}

/// Greedy casts Humility, which keeps the Angler from dealing lethal damage: the worst of the
/// two roots. The race never drops the default's root, so it is scored in every world.
#[test]
fn race_never_drops_the_defaults_root() {
    let (choice, default, raced) = search(&LETHAL.hand(P0, &[names::HUMILITY]), RACE);
    let report = raced.turn.unwrap();
    let other = Choice(1 - default.0);
    assert!(raced.means[usize::from(default.0)] < raced.means[usize::from(other.0)]);
    assert_eq!(report.dropped, 0);
    assert!(report.worlds > 48);
    assert!(raced.visits.iter().all(|&n| u64::from(n) == report.worlds));
    assert_eq!(choice, other);
}

#[test]
fn race_requires_the_budgeted_turn_search() {
    assert!(RACE.parse::<Family>().is_ok());
    assert_eq!(RACE.parse::<Family>().unwrap().to_string(), RACE);
    let err = "turn72/hidden/rollout/tuned/anchor+race"
        .parse::<Family>()
        .unwrap_err();
    assert!(err.contains("requires the method turnbudget72"), "{err}");
}
