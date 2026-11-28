use beryl_cli::blunders::{entries, reach, run_seed};
use beryl_cli::transcript::labels;
use beryl_engine::{Choice, Scratch, observe};
use beryl_search::{DEEP_SEARCHER, Family, LongSearch, Settings};

use crate::common::TEST;
use beryl_cli::why::*;
use beryl_search::{Agent, Method};

fn oust(a: &str, b: &str) -> Result<String, String> {
    run(
        &TEST,
        &["oust-own-token", "--a", a, "--b", b, "--rollouts", "8"].map(str::to_owned),
    )
}

#[test]
fn identical_choices_have_identical_rollouts() {
    let out = oust("pass", "pass").unwrap();
    assert!(
        out.contains("A - B  +0.00 pp    paired SE  0.00 pp"),
        "{out}"
    );
    assert!(out.contains("Choice\n\nA won, B lost"), "{out}");
    assert_eq!(out.matches("No such pairs.").count(), 2, "{out}");
    let score = out.split_once("Search score\nA  ").unwrap().1;
    let (a, rest) = score.split_once("    B  ").unwrap();
    let (b, rest) = rest.split_once("    A - B  ").unwrap();
    assert_eq!(a, b);
    assert!(rest.starts_with("+0.0000    paired SE  0.0000"));
}

#[test]
fn compares_oust_and_pass() {
    let out = oust("cast Oust", "pass").unwrap();
    assert!(out.contains("\nWin rate\n"));
    assert!(out.contains("Target          Rogue Token (P1)"), "{out}");
    assert!(out.contains("\nA won, B lost\n"));
    assert!(out.contains("\nB won, A lost\n"));
}

#[test]
fn an_unknown_choice_lists_candidates() {
    let err = oust("missing", "pass").unwrap_err();
    assert!(err.contains("unknown choice \"missing\""));
    assert!(err.contains("cast Oust"));
    assert!(err.contains("pass"));
}

#[test]
fn prints_both_tree_branches() {
    let out = run(
        &TEST,
        &[
            "oust-own-token",
            "--a",
            "cast Oust",
            "--b",
            "pass",
            "--rollouts",
            "1",
            "--tree",
            "--work",
            "100k",
            // The default family's turn search records no decision levels, so name a
            // resampling-tree family, the one this report lists below each branch.
            "--family",
            "pimc72/hidden/rollout/tuned/anchor+guard+prior",
        ]
        .map(str::to_owned),
    )
    .unwrap();
    let tree = out.split_once("\nTree (100000 steps").unwrap().1;
    assert!(tree.contains("\nA \"cast Oust\""), "{tree}");
    assert!(tree.contains("\nB \"pass\""), "{tree}");
    assert!(tree.contains("1. P"), "{tree}");
}

#[test]
fn tree_report_counts_the_ordinary_search_visits() {
    let name = "oust-own-token";
    let entry = entries()
        .unwrap()
        .into_iter()
        .find(|entry| entry.name == name)
        .unwrap();
    let at = reach(&TEST, &entry).unwrap();
    let mut scratch = Scratch::new();
    let obs = observe(&at.s, &at.t, &mut scratch, at.d.decider);
    let candidates = labels(&at.t, &at.s, &at.d);
    let choices = ["cast Oust", "pass"]
        .map(|name| Choice(candidates.iter().position(|label| label == name).unwrap() as u16));
    // Turn search keeps no decision levels, so only the resampling-tree methods are traced.
    for method in [Method::Pimc { worlds: 72 }, Method::Ismcts { trees: 4 }] {
        let family = Family {
            method,
            ..DEEP_SEARCHER
        };
        let settings = Settings {
            family,
            work: 100_000,
            threads: 2,
        };
        let seed = run_seed(name, 0);
        let mut ordinary = LongSearch::new(&at.t, settings, seed);
        let mut traced = LongSearch::new(&at.t, settings, seed);
        let report = traced.choose_with_tree(obs, &at.d, choices, labels);
        assert_eq!(
            report.choice,
            ordinary.choose(obs, &at.d, beryl_search::AgentDecision(0))
        );
        let search = ordinary.last_search().unwrap();
        for (branch, choice) in report.branches.into_iter().zip(choices) {
            let i = usize::from(choice.0);
            let mut parent = branch.visits.count;
            assert_eq!(parent, u64::from(search.visits[i]));
            let expected = search.rewards[i] / f64::from(search.visits[i]);
            assert!((branch.visits.mean().unwrap() - expected).abs() < 1e-12);
            assert!(!branch.levels.is_empty());
            for level in branch.levels {
                let children = level.decisions.iter().flat_map(|d| &d.children);
                assert!(children.map(|(_, visits)| visits.count).sum::<u64>() <= parent);
                let (group, child) = level.follow.unwrap();
                parent = level.decisions[group].children[child].1.count;
            }
        }
    }
}
