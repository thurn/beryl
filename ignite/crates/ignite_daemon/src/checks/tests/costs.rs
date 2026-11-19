//! What a live run records: each check's measured cost in its stored result, an adapter's flaky
//! report, results of a tree that changed during the run, and the build a `list` waits for.

use ignite_core::ids::RecordId;
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
fn a_product_that_fails_for_two_families_is_reported_once() {
    let world = jsonl_world();
    let tree_id = TreeId::new("tree-01").unwrap();
    world.builds.script(
        &tree_id,
        "deps",
        BuildResult::Failed {
            output: world.dir.join("build.txt"),
            usage: Usage::default(),
        },
    );
    let tree = world.checks.open(&world.ctx, &tree_id).unwrap().1;
    let mut run = CheckRun::default();
    let deps = vec!["deps".to_owned()];
    for _ in 0..2 {
        let who = execute::build_for(&options(false));
        let built = Checks::build_products(&world.ctx, &tree, &deps, &[], &who, &mut run);
        assert!(built.is_err());
    }
    assert_eq!(run.build_failures.len(), 1);
}

#[test]
fn list_builds_what_it_needs_before_it_runs() {
    let world = jsonl_world();
    let run = run(&world, &["unit:a"]);
    assert!(run.passed());
    let requests = world.builds.requests();
    assert!(requests.iter().any(|(_, product, _)| product == "deps"));
}

#[test]
fn each_stored_result_names_the_record_of_the_run_that_gave_it() {
    let world = jsonl_world();
    // `unit:b` fails twice: its result keeps the first failed run's output, and so its record.
    world
        .launcher
        .script("unit:b", &[Some("fail"), Some("fail")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    let records = world.hub.stored_records();
    // The fake hub numbers records from 1 in the order they arrive.
    let run_records: Vec<RecordId> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.verb == "test")
        .map(|(i, _)| RecordId(u64::try_from(i).unwrap() + 1))
        .collect();
    let lookup = |check: &str| {
        world.checks.cache().lookup(&[key_of(&world, check)])[0]
            .clone()
            .unwrap()
    };
    // The list command's record comes first, then the two runs.
    assert_eq!(run_records.len(), 3, "{records:?}");
    assert_eq!(lookup("unit:a").record, run_records[1]);
    assert_eq!(lookup("unit:b").outcome, Outcome::Fail);
    assert_eq!(lookup("unit:b").record, run_records[1]);
    assert!(run.records.contains(&(name("unit:a"), run_records[1])));
}

/// The target runner counts each test process, but only a run launched with its variables
/// records anything. Before check runs got them, a family's `cargo nextest run` recorded one
/// cost for the whole command and no test process of its own.
#[test]
fn a_nextest_run_records_its_test_processes_one_by_one() {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    assert!(run(&world, &["rs:core::a"]).passed());

    let runs = world.launcher.runs.lock().unwrap();
    let nextest = runs
        .iter()
        .find(|spec| spec.args[0] == "cargo" && spec.args.iter().any(|a| a == "run"))
        .expect("the family's run command");
    let env = |name: &str| nextest.env.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    assert!(
        nextest
            .env
            .iter()
            .any(|(k, v)| k.ends_with("_RUNNER") && v.ends_with("test-runner")),
        "the target runner is set: {:?}",
        nextest.env
    );
    assert!(env("RUSTC_WRAPPER").is_some_and(|v| v.ends_with("rustc-wrap")));
    let units_file = env(crate::wrap::UNITS_ENV).expect("the units file");
    assert!(
        !Path::new(units_file).exists(),
        "read and deleted after the run"
    );
    assert!(
        world
            .dir
            .join("shims")
            .join(crate::wrap::RUNNER_SCRIPT)
            .exists(),
        "the scripts were written"
    );
    drop(runs);

    let records = world.hub.stored_records();
    let recorded: Vec<_> = records
        .iter()
        .filter(|r| !r.compile_units.is_empty())
        .collect();
    assert_eq!(recorded.len(), 1, "{records:?}");
    let unit = &recorded[0].compile_units[0];
    assert_eq!(
        (unit.crate_name.as_str(), unit.mode.as_str()),
        ("core", "run")
    );
}

/// A landing waited for a full grant of cores before every command, even the `list` command
/// that only prints a family's checks.
#[test]
fn a_list_command_asks_for_one_core_and_a_run_for_the_settings_cores() {
    let world = jsonl_world();
    assert!(run(&world, &["unit:a"]).passed());
    let runs = world.launcher.runs.lock().unwrap().clone();
    let cores_of = |program: &str| -> Vec<u32> {
        runs.iter()
            .filter(|s| s.args[0] == program)
            .map(|s| s.cores)
            .collect()
    };
    assert_eq!(cores_of("lister"), [1]);
    assert_eq!(cores_of("runner"), [world.checks.settings.cores]);
}

#[test]
fn a_gate_run_that_is_entirely_cached_launches_no_command() {
    let world = jsonl_world();
    assert!(run(&world, &["unit:a"]).passed());
    let launched = world.launcher.runs.lock().unwrap().len();
    let again = run(&world, &["unit:a"]);
    assert!(again.passed());
    assert_eq!(again.from_cache, 1, "{again:?}");
    assert_eq!(world.launcher.runs.lock().unwrap().len(), launched);
}
