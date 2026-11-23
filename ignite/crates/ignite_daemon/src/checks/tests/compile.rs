//! A cargo-nextest run whose own Cargo build fails before any test runs.

use super::*;
use crate::build::repair::SUSPECT_FILE;

/// Cargo's lines when a test target failed to link in a tree whose incremental build was
/// damaged, shortened.
const LINK_FAILURE: &str = "\
error: linking with `cc` failed: exit status: 1
  = note: Undefined symbols for architecture arm64:
          ld: symbol(s) not found for architecture arm64
error: could not compile `core` (test \"core\") due to 1 previous error
";

const PASSED: &str = "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n";

fn nextest_world() -> World {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() = PASSED.to_owned();
    world
}

fn nextest_runs(world: &World) -> usize {
    world
        .launcher
        .commands("cargo")
        .iter()
        .filter(|args| args.iter().any(|a| a == "run"))
        .count()
}

#[test]
fn a_stopped_nextest_run_neither_retries_nor_caches_a_failure() {
    let world = nextest_world();
    let target = world.dir.join("out");
    fs::create_dir_all(&target).unwrap();
    // The supervisor has ended the command. The verb has yet to see the stop request.
    fs::write(world.dir.join("ended"), "").unwrap();
    fs::write(
        world.dir.join("result.json"),
        serde_json::to_vec(&crate::supervise::ResultFile {
            exit: Some(101),
            output: world.dir.join("output.log"),
            usage: Usage::default(),
            hung: false,
        })
        .unwrap(),
    )
    .unwrap();
    // Nextest reports a failed test for the process it cancelled with SIGTERM.
    world.launcher.nextest_run.lock().unwrap().clear();
    let cancelled = concat!(
        "  Cancelling due to signal: 1 test still running\n",
        r#"{"type":"test","event":"failed","name":"core::core$a","stdout":""}"#,
        "\n",
    );
    world
        .launcher
        .nextest_failures
        .lock()
        .unwrap()
        .push_back(cancelled.to_owned());

    let run = run(&world, &["rs:core::a"]);

    assert!(!run.passed());
    assert_eq!(run.skipped, 1);
    assert!(run.outcomes.is_empty());
    assert_eq!(nextest_runs(&world), 1);
    assert_eq!(cached(&world, "rs:core::a"), None);
    assert!(target.join(SUSPECT_FILE).is_file());
}

/// The family `rs` needs the product `deps`, whose target directory is `out`.
fn incremental(world: &World) -> PathBuf {
    let incremental = world.dir.join("out").join("debug").join("incremental");
    fs::create_dir_all(
        incremental
            .join("core-1imqhd34qdwcf")
            .join("s-hoa6uqen2b-0h7-abc"),
    )
    .unwrap();
    fs::create_dir_all(incremental.join("other-3nuzzneutc6f3")).unwrap();
    incremental
}

/// In one landing tree, every run of the Rust checks failed to link a test target in a few
/// seconds; the checks were counted as unreported, held off for an hour, and `master` waited.
/// The crate's incremental work is now cleared and the run made once more, which passes.
#[test]
fn a_run_whose_build_fails_to_link_is_cleared_and_run_once_more() {
    let world = nextest_world();
    let incremental = incremental(&world);
    world
        .launcher
        .nextest_failures
        .lock()
        .unwrap()
        .push_back(LINK_FAILURE.to_owned());

    let run = run(&world, &["rs:core::a"]);
    assert!(run.passed(), "{run:?}");
    assert_eq!(outcome(&run, "rs:core::a"), Outcome::Pass);
    assert_eq!(nextest_runs(&world), 2);
    assert!(!incremental.join("core-1imqhd34qdwcf").exists());
    assert!(incremental.join("other-3nuzzneutc6f3").exists());
}

/// A build that still fails is a failed build: its checks wait on it, are not run twice more
/// as unreported checks would be, and say so with the reason tip verification never holds off.
#[test]
fn a_run_whose_build_keeps_failing_leaves_its_checks_waiting_on_the_build() {
    let world = nextest_world();
    incremental(&world);
    for _ in 0..4 {
        world
            .launcher
            .nextest_failures
            .lock()
            .unwrap()
            .push_back(LINK_FAILURE.to_owned());
    }
    let run = run(&world, &["rs:core::a"]);
    assert_eq!(outcome(&run, "rs:core::a"), Outcome::Incomplete);
    assert_eq!(nextest_runs(&world), 2, "one repair, then no retries");
    assert_eq!(run.build_failures.len(), 1, "{:?}", run.build_failures);
    assert_eq!(run.build_failures[0].0, "deps");
    let failure = run.failures.iter().find(|f| f.check == name("rs:core::a"));
    let failure = failure.expect("the check's failure");
    assert_eq!(failure.reason, Some(IncompleteReason::BuildFailed));
    assert_eq!(failure.message.as_deref(), Some(BUILD_FAILED));
}

/// A source error is not repaired: it fails the same way after a clean build.
#[test]
fn a_run_whose_build_has_a_source_error_is_not_run_again() {
    let world = nextest_world();
    let incremental = incremental(&world);
    world.launcher.nextest_failures.lock().unwrap().push_back(
        "error[E0308]: mismatched types\n\
         error: could not compile `core` (lib) due to 1 previous error\n"
            .to_owned(),
    );
    let run = run(&world, &["rs:core::a"]);
    assert_eq!(outcome(&run, "rs:core::a"), Outcome::Incomplete);
    assert_eq!(nextest_runs(&world), 1);
    assert!(incremental.join("core-1imqhd34qdwcf").exists());
}

/// A run ended partway marks its target directory, and the next run clears the crates whose
/// compile it left unfinished before it starts.
#[test]
fn a_run_after_one_ended_partway_clears_its_unfinished_compiles() {
    let world = nextest_world();
    let incremental = incremental(&world);
    let target = world.dir.join("out");
    fs::write(target.join(SUSPECT_FILE), "").unwrap();
    fs::create_dir_all(
        incremental
            .join("other-3nuzzneutc6f3")
            .join("s-1-2-working"),
    )
    .unwrap();

    let run = run(&world, &["rs:core::a"]);
    assert!(run.passed(), "{run:?}");
    assert!(!incremental.join("other-3nuzzneutc6f3").exists());
    assert!(incremental.join("core-1imqhd34qdwcf").exists());
    assert!(!target.join(SUSPECT_FILE).exists());
}
