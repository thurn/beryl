//! What a live run records: each check's measured cost in its stored result, an adapter's flaky
//! report, results of a tree that changed during the run, and the build a `list` waits for.

use ignite_core::protocol::hub::NewResult;
use ignite_core::traits::BuildResult;

use super::*;

#[test]
fn each_stored_result_carries_its_share_of_the_measured_cost() {
    let world = jsonl_world();
    world.hub.set_unreachable(true);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert!(run.passed());
    let queued = world.checks.cache().queued();
    let results: Vec<NewResult> = queued
        .iter()
        .filter(|(kind, _)| kind == "result")
        .map(|(_, body)| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(results.len(), 2);
    for result in results {
        // One run of two checks used 6 billion instructions and 2 CPU seconds.
        assert_eq!(result.cost.instructions, Some(3_000_000_000));
        assert_eq!(result.cost.cpu_ms, 1000);
    }
}

#[test]
fn a_flaky_report_from_the_adapter_is_reported_and_not_cached() {
    let world = jsonl_world();
    world.launcher.script("unit:b", &[Some("flaky")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert!(run.passed());
    assert_eq!(outcome(&run, "unit:b"), Outcome::Flaky);
    assert_eq!(world.hub.flaky_reports().len(), 1);
    assert_eq!(cached(&world, "unit:b"), None);
    assert_eq!(cached(&world, "unit:a"), Some(Outcome::Pass));
}

#[test]
fn a_result_is_not_stored_when_the_tree_changed_during_the_run() {
    let world = jsonl_world();
    let git = world.git.clone();
    *world.launcher.on_run.lock().unwrap() = Some(Box::new(move || {
        git.set_object("src", Oid::new("b".repeat(40)).unwrap());
    }));
    let before = key_of(&world, "unit:a");
    let run = run(&world, &["unit:a"]);
    assert_eq!(
        outcome(&run, "unit:a"),
        Outcome::Pass,
        "the run still reports it"
    );
    *world.launcher.on_run.lock().unwrap() = None;
    assert!(
        world.checks.cache().lookup(&[before])[0].is_none(),
        "nothing is stored under the hash taken before the edit"
    );
}

#[test]
fn list_waits_for_its_products_and_a_failed_build_is_not_an_incomplete_family() {
    let world = jsonl_world();
    let tree = TreeId::new("tree-01").unwrap();
    world.builds.script(
        &tree,
        "deps",
        BuildResult::Failed {
            output: world.dir.join("build.txt"),
            usage: Usage::default(),
        },
    );
    let run = run(&world, &["unit:a"]);
    assert_eq!(run.build_failures.len(), 1);
    assert!(run.incomplete_families.is_empty());
    assert!(
        world.launcher.commands("lister").is_empty(),
        "list never ran"
    );
    assert!(!run.passed());
}

#[test]
fn list_builds_what_it_needs_before_it_runs() {
    let world = jsonl_world();
    let run = run(&world, &["unit:a"]);
    assert!(run.passed());
    let requests = world.builds.requests();
    assert!(requests.iter().any(|(_, product, _)| product == "deps"));
}
