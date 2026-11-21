use std::collections::BTreeSet;

use ignite_core::gate::{GateCheck, History};
use ignite_core::ids::CheckName;
use ignite_core::protocol::hub::{AreaStats, CheckState, CheckStats, CheckStatsReply};
use ignite_core::score::{AreaRecord, Cost};
use ignite_core::time::Timestamp;
use ignite_store::import::runlog::Catch;

use super::{KEEP, gate_checks, replay, report};

/// A check with `runs` runs and `failures` failures after changes to each of `areas`, costing
/// `cost` billion instructions, in a family that never fails otherwise.
fn check(name: &str, areas: &[&str], runs: u32, failures: u32, cost: f64) -> GateCheck {
    GateCheck {
        family_startup: Cost::Billions(0.0),
        check: CheckName::new(name).unwrap(),
        always: false,
        edited: false,
        cached: false,
        history: Some(History {
            areas: areas
                .iter()
                .map(|a| ((*a).to_owned(), AreaRecord { runs, failures }))
                .collect(),
            median_cost: Cost::Billions(cost),
        }),
        family_rate: 0.0,
        family_median_cost: Cost::Billions(cost),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

fn catch(check: &str, areas: &[&str]) -> Catch {
    Catch {
        check: check.to_owned(),
        at: Timestamp::EPOCH,
        areas: areas.iter().map(|a| (*a).to_owned()).collect(),
    }
}

/// Whether `kept` of `total` catches is at least [`KEEP`].
fn enough(kept: usize, total: usize) -> bool {
    kept as f64 >= KEEP * total as f64
}

#[test]
fn twenty_catches_keep_nineteen_and_the_cap_covers_their_gates() {
    // Check `c{f}` failed f times in 100 runs, so the scores, with the catch itself left out, are 0/119 .. 19/119 per billion.
    let checks: Vec<GateCheck> = (1..=20)
        .map(|f| check(&format!("rust:c{f:02}"), &["a"], 100, f, 1.0))
        .collect();
    let catches: Vec<Catch> = (1..=20)
        .map(|f| catch(&format!("rust:c{f:02}"), &["a"]))
        .collect();
    let found = replay(&catches, &checks).unwrap();
    assert_eq!(found.catches, 20);
    assert!((found.chosen().threshold - 1.0 / 119.0).abs() < 1e-12);
    assert_eq!(found.chosen().kept, 19);
    // Every catch touched the same area, so each gate runs the same 19 one-billion checks.
    assert!((found.chosen().spent - 19.0).abs() < 1e-9);
    let kept: Vec<usize> = found.nearby.iter().map(|r| r.kept).collect();
    assert_eq!(kept, vec![17, 18, 19, 20]);
    assert!((found.chosen().spent.ceil() - 19.0).abs() < f64::EPSILON);
    assert!(found.threshold() <= found.chosen().threshold);
}

#[test]
fn on_a_seeded_history_the_threshold_keeps_95_percent_and_the_next_larger_keeps_fewer() {
    // A small linear congruential generator, so the history is the same on every run.
    let mut seed: u64 = 0x5eed;
    let mut next = |below: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % below
    };
    let areas = ["core", "rules", "web/src/", "tools/x/"];
    let mut checks = Vec::new();
    for i in 0..60 {
        let mine: Vec<&str> = areas.iter().copied().filter(|_| next(2) == 0).collect();
        let runs = 5 + next(200) as u32;
        let failures = next(u64::from(runs) / 4 + 1) as u32;
        let cost = 0.1 + next(500) as f64 / 10.0;
        checks.push(check(&format!("rust:t{i:02}"), &mine, runs, failures, cost));
    }
    let mut catches = Vec::new();
    for _ in 0..237 {
        let caught = format!("rust:t{:02}", next(60));
        let touched: Vec<&str> = areas.iter().copied().filter(|_| next(3) == 0).collect();
        catches.push(catch(&caught, &touched));
    }
    // A catch of a check the hub has no record of cannot be scored.
    catches.push(catch("rust:gone", &["core"]));

    let found = replay(&catches, &checks).unwrap();
    assert_eq!((found.catches, found.unscored), (237, 1));
    assert!(enough(found.chosen().kept, found.catches));
    let larger = &found.nearby[found.chosen - 1];
    assert!(larger.threshold > found.chosen().threshold);
    assert!(!enough(larger.kept, found.catches), "{larger:?}");
    // A lower threshold runs at least as much.
    for pair in found.nearby.windows(2) {
        assert!(pair[0].spent <= pair[1].spent && pair[0].kept <= pair[1].kept);
    }
    let text = report(&found, 14).text;
    assert!(text.contains("[gate]\nthreshold = "), "{text}");
}

#[test]
fn nothing_to_replay_without_a_scored_catch() {
    let checks = vec![check("rust:a", &["core"], 10, 1, 1.0)];
    assert!(replay(&[], &checks).is_none());
    assert!(replay(&[catch("rust:b", &["core"])], &checks).is_none());
}

#[test]
fn hub_records_become_gate_checks_without_switched_off_checks_or_left_out_families() {
    let stats = |name: &str, state, median_cost, unit: &str| CheckStats {
        check: CheckName::new(name).unwrap(),
        family: name.split(':').next().unwrap().to_owned(),
        state,
        areas: vec![AreaStats {
            area: "core".to_owned(),
            runs: 4,
            failures: 1,
        }],
        median_cost,
        unit: unit.to_owned(),
    };
    let replies = vec![
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "rust".to_owned(),
            family_rate: 0.1,
            runs: None,
            checks: vec![
                stats("rust:old", CheckState::On, Some(2.0), "cpu_s"),
                stats("rust:new", CheckState::Uncached, None, ""),
                stats("rust:off", CheckState::Off, Some(9.0), "instructions"),
            ],
        },
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "fmt".to_owned(),
            family_rate: 0.0,
            runs: None,
            checks: vec![stats("fmt:all", CheckState::On, Some(1.0), "instructions")],
        },
    ];
    let left_out: BTreeSet<String> = ["fmt".to_owned()].into();
    let checks = gate_checks(&replies, &left_out);
    let names: Vec<&str> = checks.iter().map(|c| c.check.as_str()).collect();
    assert_eq!(names, vec!["rust:old", "rust:new"]);
    let old = checks[0].history.as_ref().unwrap();
    assert_eq!(old.median_cost, Cost::CpuSeconds(2.0));
    // The family's median counts the switched-off check too: 2 s at 17 billion/s, and 9.
    assert_eq!(checks[0].family_median_cost, Cost::Billions(21.5));
    // A check with runs but no cost is charged its family's median.
    let new = checks[1].history.as_ref().unwrap();
    assert_eq!(new.median_cost, checks[1].family_median_cost);
    assert_eq!(
        new.areas["core"],
        AreaRecord {
            runs: 4,
            failures: 1
        }
    );
}

#[test]
fn a_catch_is_scored_without_its_own_failure() {
    // The hub counts 10 failures in 100 runs, one of them this catch's: the gate could only have
    // known 9 in 99.
    let checks = [check("rust:c", &["a"], 100, 10, 1.0)];
    let found = replay(&[catch("rust:c", &["a"])], &checks).unwrap();
    assert!((found.chosen().threshold - 9.0 / (99.0 + 20.0 * 0.0)).abs() > 1e-9);
    assert!((found.chosen().threshold - 9.0 / 119.0).abs() < 1e-12);
}

#[test]
fn the_cap_costs_only_checks_the_change_could_affect() {
    let checks = [
        check("rust:near", &["a"], 100, 50, 2.0),
        check("rust:far", &["b"], 100, 50, 1000.0),
    ];
    let found = replay(&[catch("rust:near", &["a"])], &checks).unwrap();
    assert!(
        (found.chosen().spent - 2.0).abs() < 1e-9,
        "{:?}",
        found.chosen()
    );
}
