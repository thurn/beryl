//! Runs that end without an ordinary verdict: a command that cannot run here, a command that
//! hangs, a cached failure from another machine, and the hang limit and class each command is
//! given.

use ignite_core::ids::RecordId;
use ignite_core::protocol::hub::IncompleteReason;

use super::*;

fn failure<'a>(run: &'a CheckRun, check: &str) -> &'a Failure {
    run.failures
        .iter()
        .find(|f| f.check.as_str() == check)
        .unwrap()
}

#[test]
fn a_check_that_cannot_run_here_is_incomplete_and_not_retried() {
    let world = jsonl_world();
    world.launcher.script("unit:b", &[Some("cannot-run")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Incomplete);
    assert_eq!(world.launcher.commands("runner").len(), 1, "not run again");
    let failure = failure(&run, "unit:b");
    assert_eq!(failure.reason, Some(IncompleteReason::CannotRun));
    let message = failure.message.as_deref().unwrap();
    assert!(message.contains("cannot run on m-1") && message.contains("no simulator"));
    assert_eq!(cached(&world, "unit:b"), None);
}

#[test]
fn a_hung_command_leaves_its_unreported_checks_incomplete_and_is_not_run_again() {
    let world = jsonl_world();
    // `unit:a` reported before the command hung on `unit:b`.
    world.launcher.script("unit:b", &[Some("hang")]);
    let run = run(&world, &["unit:a", "unit:b"]);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Incomplete);
    // The hung command ended right after `unit:a`'s line, which might have been the first of
    // several steps, so `unit:a` ran again alone and passed. `unit:b` was not run again.
    let commands = world.launcher.commands("runner");
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[1][1..], ["unit:a"]);
    let failure = failure(&run, "unit:b");
    assert_eq!(failure.reason, Some(IncompleteReason::Hung));
    let output = PathBuf::from(failure.output.as_deref().unwrap());
    assert!(
        output.with_extension("keep").exists(),
        "an incomplete run's output is kept for 14 days"
    );
    assert_eq!(cached(&world, "unit:b"), None);
    assert_eq!(cached(&world, "unit:a"), Some(Outcome::Pass));
}

#[test]
fn a_failure_whose_rerun_hangs_stays_a_failure() {
    let world = jsonl_world();
    world
        .launcher
        .script("unit:b", &[Some("fail"), Some("hang")]);
    let run = run(&world, &["unit:b"]);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Fail);
    assert_eq!(world.launcher.commands("runner").len(), 2);
}

#[test]
fn a_passing_run_keeps_its_output_only_a_day() {
    let world = jsonl_world();
    run(&world, &["unit:a"]);
    let flaky = jsonl_world();
    flaky
        .launcher
        .script("unit:a", &[Some("fail"), Some("pass")]);
    run(&flaky, &["unit:a"]);
    let marked = |world: &World| {
        fs::read_dir(&world.dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "keep"))
            .count()
    };
    assert_eq!(marked(&world), 0);
    assert_eq!(
        marked(&flaky),
        1,
        "the failed first run of a flaky check is kept"
    );
}

/// Stores a failure of `check` at the hub as `machine` produced it.
fn hub_failure(world: &World, check: &str, machine: &str) {
    world
        .hub
        .store(&[NewResult {
            key: key_of(world, check),
            outcome: Outcome::Fail,
            machine: MachineId::new(machine).unwrap(),
            record: RecordId(7),
            output: Some("/elsewhere/output.log".to_owned()),
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        }])
        .unwrap();
}

#[test]
fn a_cached_failure_from_another_machine_runs_here_and_a_pass_replaces_it() {
    let world = jsonl_world();
    hub_failure(&world, "unit:a", "m-2");
    let run = run(&world, &["unit:a"]);
    assert!(run.passed());
    assert_eq!((run.ran, run.from_cache), (1, 0));
    assert_eq!(cached(&world, "unit:a"), Some(Outcome::Pass));
    // The hub now holds this machine's pass; storing it is how the hub learns of the
    // disagreement and files a ticket for the machine that failed.
    let at_hub = world.hub.lookup(&[key_of(&world, "unit:a")]).unwrap();
    let at_hub = at_hub[0].as_ref().unwrap();
    assert_eq!(
        (at_hub.outcome, at_hub.machine.as_str()),
        (Outcome::Pass, "m-1")
    );
}

#[test]
fn a_cached_failure_from_this_machine_is_trusted() {
    let world = jsonl_world();
    hub_failure(&world, "unit:a", "m-1");
    let run = run(&world, &["unit:a"]);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Fail);
    assert_eq!((run.ran, run.from_cache), (0, 1));
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn a_cached_failure_from_another_machine_that_fails_here_is_stored_as_this_machines() {
    let world = jsonl_world();
    hub_failure(&world, "unit:a", "m-2");
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    let run = run(&world, &["unit:a"]);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Fail);
    let hit = world.checks.cache().lookup(&[key_of(&world, "unit:a")]);
    assert_eq!(hit[0].as_ref().unwrap().machine.as_str(), "m-1");
}

#[test]
fn each_command_is_told_its_class_and_given_a_hang_limit_from_its_slowest_pass() {
    let world = jsonl_world();
    run(&world, &["unit:a"]);
    run(&world, &["unit:b"]);
    let specs = world.launcher.runs.lock().unwrap().clone();
    let limits: Vec<(String, Option<u64>)> = specs
        .iter()
        .map(|s| (s.args[0].clone(), s.hang_limit_s))
        .collect();
    // With no pass on record a command may run 30 minutes. The script's commands take a
    // second, and four times that is under the 15-minute floor.
    assert_eq!(
        limits,
        [
            ("lister".to_owned(), Some(1800)),
            ("runner".to_owned(), Some(1800)),
            // The second run is answered from the kept listing.
            ("runner".to_owned(), Some(900)),
        ]
    );
    assert!(specs.iter().all(|s| {
        s.env
            .contains(&("IGNITE_CLASS".to_owned(), "agent".to_owned()))
    }));
}

#[test]
fn the_hang_limit_is_four_times_the_slowest_pass_within_its_bounds() {
    assert_eq!(hang_limit_s(None), 30 * 60);
    assert_eq!(hang_limit_s(Some(60.0)), 15 * 60);
    assert_eq!(hang_limit_s(Some(300.0)), 20 * 60);
    assert_eq!(hang_limit_s(Some(3600.0)), 40 * 60);
}
