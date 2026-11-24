use beryl_cli::match_log::AnchorRecord;
use beryl_engine::Choice;
use beryl_search::{Knowledge, PolicyWeights, RolloutPolicy, Rules};

use beryl_cards::{CARDS, names};
use beryl_engine::{Step, TurnStep};
use beryl_scenario::{Action, Live, P0, Scenario};
use beryl_search::Family;

use beryl_cli::fit::*;

#[test]
fn fit_corrects_an_overvalued_land_deterministically() {
    let scenario = Scenario::new("choice of land")
        .at(4, P0, TurnStep::PrecombatMain)
        .hand(P0, &[names::ISLAND, names::TUNDRA]);
    let mut live = Live::new(&CARDS, &scenario).unwrap();
    let Step::Decide(decision) = live.position(Action::pass(P0)).unwrap() else {
        panic!("expected a land play");
    };
    let table = live.table();
    let label = (0..decision.len())
        .map(|i| Choice(i as u16))
        .find(|&c| live.name_of(&decision, c) == Some(names::TUNDRA))
        .unwrap();
    let sample = Sample {
        state: *live.state(),
        decision,
        label,
    };
    let coordinate = Coordinate {
        name: names::TUNDRA,
        base: 14,
        radius: 10,
        step: 2,
    };
    let score = |values: &[i32]| {
        let policy = RolloutPolicy::with_fit(
            table,
            Rules::default(),
            PolicyWeights::TUNED,
            &[(coordinate.name, values[0] as i16)],
        );
        16 * agreement(&policy, table, std::slice::from_ref(&sample))
    };
    assert_eq!(score(&[coordinate.base]), 0);
    let values = fit(&[coordinate], score);
    assert!(values[0] < coordinate.base);
    assert_eq!(score(&values), 16);
    assert_eq!(values, fit(&[coordinate], score));
}

#[test]
fn a_search_pick_must_pass_the_paired_check_and_be_played() {
    let a = AnchorRecord {
        greedy: 0,
        search: 1,
        lead: 0.06,
        error: 0.01,
        trees: 0,
        pairs: Some(20),
    };
    assert!(confirmed(a, 1));
    assert!(!confirmed(a, 0));
    assert!(!confirmed(AnchorRecord { pairs: None, ..a }, 1));
    assert!(!confirmed(AnchorRecord { error: 0.04, ..a }, 1));
}

#[test]
fn fitted_family_round_trips() {
    let name = "pimc72/hidden/rollout/fitted/anchor+guard+hold+waste+combat+sure+paired";
    let family = name.parse::<Family>().unwrap();
    assert_eq!(family.knowledge, Knowledge::Fitted);
    assert_eq!(family.to_string(), name);
}
