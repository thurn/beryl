use std::collections::BTreeMap;
use std::path::PathBuf;

use beryl_cli::ladder::{Bar, Match, run};
use beryl_cli::value::report::Report;
use beryl_cli::value::{PREDICTORS, Sample, folds, main, predict};
use beryl_engine::rng::SplitMix64;
use beryl_search::sigmoid;

use crate::common::TEST;

const ENTRANT: &str = "pimc18/hidden/rollout/tuned@3k";

/// Samples whose chance of a win is `sigmoid(2.5 x + 3 x y)` in two features: partly linear,
/// partly not. The search's estimate is a flat one half, so `search` is a constant.
fn synthetic(n: usize) -> Vec<Sample> {
    let mut rng = SplitMix64::new(11);
    let mut uniform = || (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    (0..n)
        .map(|i| {
            let (x, y) = (4.0 * uniform() - 2.0, 4.0 * uniform() - 2.0);
            let won = uniform() < sigmoid(2.5 * x + 3.0 * x * y);
            Sample {
                group: format!("g{}", i / 10),
                game: format!("0/{}", i / 10),
                budget: 3_000,
                features: vec![x, y],
                search: 0.5,
                won,
            }
        })
        .collect()
}

#[test]
fn a_network_learns_what_a_regression_cannot() {
    let samples = synthetic(3_000);
    let report = Report::new(&samples, &predict(&samples, 1).unwrap(), 0, 1);
    let loss: BTreeMap<&str, f64> = report
        .predictors
        .iter()
        .map(|p| (p.name, p.overall.log_loss))
        .collect();
    assert!(loss["mlp"] < loss["linear"] - 0.02, "{loss:?}");
    assert!(loss["linear"] < loss["search"] - 0.02, "{loss:?}");
    assert!(report.text().lines().count() <= 20);
}

#[test]
fn no_group_spans_two_folds() {
    let samples = synthetic(500);
    let mut seen = BTreeMap::new();
    for (s, f) in samples.iter().zip(folds(&samples, 7)) {
        assert_eq!(*seen.entry(s.group.clone()).or_insert(f), f, "{}", s.group);
    }
    let used: std::collections::BTreeSet<usize> = seen.values().copied().collect();
    assert_eq!(used.len(), 5);
}

/// Over a four-game match of a search against itself, the command reports every predictor,
/// and a second run from the same seed writes the same `value.json`.
#[test]
fn value_test_runs_over_a_match_and_repeats() {
    let base = std::env::temp_dir().join(format!("beryl-value-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("match");
    let m = Match {
        challenger: ENTRANT.to_owned(),
        champion: ENTRANT.to_owned(),
        work: 3_000,
        bar: Bar::Audit,
        cap: 4,
        seed: 3,
        out: dir.clone(),
        positions: Vec::new(),
    };
    run(&TEST, &m, |_, _| {}).unwrap();
    let written = |name: &str| -> serde_json::Value {
        let out: PathBuf = base.join(name);
        let args = [
            dir.display().to_string(),
            "--seed".into(),
            "5".into(),
            "--out".into(),
        ]
        .into_iter()
        .chain([out.display().to_string()])
        .collect::<Vec<_>>();
        main(&TEST, &args).unwrap();
        serde_json::from_str(&std::fs::read_to_string(out.join("value.json")).unwrap()).unwrap()
    };
    let first = written("one");
    assert_eq!(first, written("two"));
    let names: Vec<&str> = first["predictors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, PREDICTORS);
    assert!(first["samples"].as_u64().unwrap() > 0, "{first}");
    assert_eq!(first["differences"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&base);
}
