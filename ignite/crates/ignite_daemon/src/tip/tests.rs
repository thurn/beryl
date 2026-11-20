use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::config::{Config, Os};
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId, SessionId, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{IncompleteReason, Landed, NewResult, ResultKey, TipClaim};
use ignite_core::protocol::tickets::LandedWork;
use ignite_core::time::Timestamp;
use ignite_core::tipround::UNMEASURED_BATCH;
use ignite_core::traits::{Git, Hub};

use super::*;

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"
[hub]
url = "http://localhost:1"
project = "test"
[gate]
threshold = 0.004
seconds = 60
[machine.local]
ticket_trees = 1
landing_trees = 1
# A Windows machine that runs every family, so checks keyed for Windows wait on a machine that
# can run them.
[machine.windows]
os = "windows"
ticket_trees = 1
landing_trees = 1
"#;

fn commit(text: &str) -> Commit {
    Commit::new(text).unwrap()
}

fn name(text: &str) -> CheckName {
    CheckName::new(text).unwrap()
}

fn ticket(text: &str) -> TicketId {
    TicketId::new(text).unwrap()
}

/// A check in family `unit` whose input hash is `hash` repeated, reading `src/<check>`.
fn check(text: &str, hash: char) -> TipCheck {
    let check = name(&format!("unit:{text}"));
    TipCheck {
        key: ResultKey {
            check: check.clone(),
            hash: InputHash::new(hash.to_string().repeat(64)).unwrap(),
            platform: None,
        },
        check,
        family: "unit".to_owned(),
        inputs: vec![format!("src/{text}")],
        runnable: true,
        reuse: true,
    }
}

mod fake;
use fake::FakeWork;

struct World {
    clock: Arc<FakeClock>,
    hub: Arc<FakeHub>,
    git: Arc<FakeGit>,
    work: Arc<FakeWork>,
    launcher: Arc<FakeLauncher>,
}

impl World {
    fn new() -> World {
        let world = World {
            clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
            hub: Arc::new(FakeHub::default()),
            git: Arc::new(FakeGit::default()),
            work: Arc::new(FakeWork::default()),
            launcher: Arc::new(FakeLauncher::default()),
        };
        world.git.set_tip("master", commit("aaaa000"));
        world
    }

    fn ctx(&self, machine: &str) -> Context {
        Context {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            repo: PathBuf::from("/repo"),
            machine: MachineId::new(machine).unwrap(),
            clock: self.clock.clone(),
            hub: self.hub.clone(),
            git: self.git.clone(),
            launcher: self.launcher.clone(),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        }
    }

    fn verifier(&self) -> TipVerifier {
        TipVerifier::new(self.work.clone(), "test").with_host("jarvis")
    }

    fn store(&self, key: &ResultKey, outcome: Outcome, machine: &str) {
        self.hub
            .store(&[NewResult {
                key: key.clone(),
                outcome,
                machine: MachineId::new(machine).unwrap(),
                record: RecordId(1),
                output: None,
                cost: ignite_core::protocol::hub::ResultCost::default(),
                areas: Vec::new(),
            }])
            .unwrap();
    }

    fn master(&self) -> Commit {
        self.git.tip("master").unwrap()
    }

    /// Records at the hub that `ticket_id` landed `commits`.
    fn record_landing(&self, ticket_id: &str, commits: &[&str]) {
        self.hub
            .landed(&Landed {
                ticket: ticket(ticket_id),
                machine: MachineId::new("m-1").unwrap(),
                session: SessionId::new("s-1").unwrap(),
                work: LandedWork {
                    commits: commits.iter().map(|c| commit(c)).collect(),
                    files: Vec::new(),
                    note: String::new(),
                },
                fresh: false,
            })
            .unwrap();
    }

    /// Makes `at` the newest `staging` commit.
    fn land(&self, at: &Commit) {
        self.git.set_tip("staging", at.clone());
    }
}

#[test]
fn a_tip_whose_results_are_all_cached_is_verified_without_running_anything() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = vec![check("a", 'a'), check("b", 'b')];
    for c in &checks {
        world.store(&c.key, Outcome::Pass, "m-2");
    }
    world.work.set_checks(&tip, checks);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert!(world.work.runs().is_empty());
    assert!(world.hub.tip_claims().is_empty());
    assert_eq!(world.master(), tip);
    let records = world.hub.stored_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].verb, "cached");
    assert_eq!(records[0].family.as_deref(), Some("unit"));
    assert!(records[0].wall_s.abs() < f64::EPSILON);
    assert_eq!(
        world.hub.tip_states().last().unwrap().state,
        TipState::Verified
    );
}

#[test]
fn two_machines_never_run_the_same_batch() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks: Vec<TipCheck> = (0..UNMEASURED_BATCH + 10)
        .map(|i| {
            let mut c = check(&format!("t{i:03}"), 'c');
            c.key.hash = InputHash::new(format!("{i:064x}")).unwrap();
            c
        })
        .collect();
    world.work.set_checks(&tip, checks);
    // Machine m-2 has claimed the first batch and is still running it.
    let first = TipClaim {
        commit: tip.clone(),
        family: "unit".to_owned(),
        batch: 0,
        machine: MachineId::new("m-2").unwrap(),
    };
    assert!(world.hub.tip_claim(&first).unwrap().granted);

    let one = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();
    let TipOutcome::Waiting {
        checks: waiting, ..
    } = one
    else {
        panic!("expected m-1 to wait, got {one:?}");
    };
    assert_eq!(waiting.len(), UNMEASURED_BATCH);
    assert_eq!(world.master(), commit("aaaa000"));

    let two = world.verifier().verify(&world.ctx("m-2"), &tip).unwrap();
    assert_eq!(two, TipOutcome::Verified);
    assert_eq!(world.master(), tip);

    let runs = world.work.runs();
    let ran_by = |machine: &str| -> Vec<CheckName> {
        runs.iter()
            .filter(|r| r.0.as_str() == machine)
            .flat_map(|r| r.2.clone())
            .collect()
    };
    let (by_one, by_two) = (ran_by("m-1"), ran_by("m-2"));
    assert_eq!(by_one.len(), 10);
    assert_eq!(by_two.len(), UNMEASURED_BATCH);
    assert!(by_one.iter().all(|c| !by_two.contains(c)));
}

#[test]
fn master_stays_behind_a_failing_check_and_moves_at_the_tip_that_fixes_it() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut verifier = world.verifier();
    world.work.add_landing("tk.21", &["bbbb000"], &["src/a"]);

    let broken = commit("bbbb000");
    world.land(&broken);
    world.work.set_checks(&broken, vec![check("a", 'a')]);
    world
        .work
        .set_outcome(&broken, &name("unit:a"), Outcome::Fail);
    assert_eq!(
        verifier.verify(&ctx, &broken).unwrap(),
        TipOutcome::Failed {
            checks: vec![name("unit:a")]
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));
    assert_eq!(world.hub.regressions().len(), 1);

    // A later landing leaves the check's inputs alone: its cached failure stands, and is not
    // reported again.
    let unrelated = commit("cccc000");
    world.land(&unrelated);
    world.work.set_checks(&unrelated, vec![check("a", 'a')]);
    world
        .work
        .set_outcome(&unrelated, &name("unit:a"), Outcome::Fail);
    assert!(matches!(
        verifier.verify(&ctx, &unrelated).unwrap(),
        TipOutcome::Failed { .. }
    ));
    assert_eq!(world.master(), commit("aaaa000"));
    assert_eq!(world.hub.regressions().len(), 1);

    // The fix changes the check's inputs, and it passes.
    let fixed = commit("dddd000");
    world.land(&fixed);
    world.work.set_checks(&fixed, vec![check("a", 'f')]);
    assert_eq!(verifier.verify(&ctx, &fixed).unwrap(), TipOutcome::Verified);
    assert_eq!(world.master(), fixed);
}

#[test]
fn the_one_landing_that_touched_a_failed_checks_inputs_is_blamed_without_a_run() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("cccc000");
    world.land(&tip);
    world
        .work
        .add_landing("tk.1", &["bbbb000", "bbbb111"], &["docs/readme"]);
    world
        .work
        .add_landing("tk.2", &["cccc000"], &["src/a/lib.rs"]);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert!(matches!(outcome, TipOutcome::Failed { .. }));
    // The only run is the tip's own.
    assert_eq!(world.work.runs().len(), 1);
    let report = &world.hub.regressions()[0];
    assert_eq!(report.caused_by, Some(ticket("tk.2")));
    assert_eq!(report.first_bad, commit("cccc000"));
    assert_eq!(report.last_good, commit("bbbb111"));
    assert!(report.suspects.is_empty());
    assert_eq!(report.search, Some(CulpritSearch::OneCandidate));
    assert_eq!(report.output, "unit:a failed");
    assert_eq!(
        world.hub.tip_states().last().unwrap().state,
        TipState::Failed
    );
}

#[test]
fn three_candidates_are_narrowed_in_two_runs() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    world.work.add_landing("tk.1", &["bbbb000"], &["src/a"]);
    world.work.add_landing("tk.2", &["cccc000"], &["src/a"]);
    world.work.add_landing("tk.3", &["dddd000"], &["src/a"]);
    let failing = check("a", 'a');
    world
        .work
        .set_outcome(&commit("bbbb000"), &failing.check, Outcome::Pass);
    world
        .work
        .set_outcome(&commit("cccc000"), &failing.check, Outcome::Fail);

    let culprit = world
        .verifier()
        .find_culprit(&ctx, &failing, &commit("aaaa000"), &commit("dddd000"))
        .unwrap()
        .unwrap();

    assert_eq!(culprit.runs, 2);
    assert_eq!(culprit.suspects.len(), 1);
    assert_eq!(culprit.suspects[0].ticket, Some(ticket("tk.2")));
    assert_eq!(culprit.first_bad, commit("cccc000"));
    assert_eq!(culprit.last_good, commit("bbbb000"));
}

#[test]
fn a_search_that_cannot_narrow_reports_the_range() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    world.work.add_landing("tk.1", &["bbbb000"], &["src/a"]);
    world.work.add_landing("tk.2", &["cccc000"], &["src/a"]);
    world.work.add_landing("tk.3", &["dddd000"], &["src/a"]);
    let failing = check("a", 'a');
    world
        .work
        .set_outcome(&commit("bbbb000"), &failing.check, Outcome::Incomplete);
    world
        .work
        .set_checks(&commit("dddd000"), vec![failing.clone()]);
    world
        .work
        .set_outcome(&commit("dddd000"), &failing.check, Outcome::Fail);
    world.land(&commit("dddd000"));

    world.verifier().verify(&ctx, &commit("dddd000")).unwrap();

    let report = &world.hub.regressions()[0];
    assert_eq!(report.caused_by, None);
    assert_eq!(
        report.suspects,
        vec![ticket("tk.1"), ticket("tk.2"), ticket("tk.3")]
    );
    assert_eq!(
        report.search,
        Some(CulpritSearch::EndedEarly {
            runs: 1,
            candidates: 3,
            why: "the run at bbbb000 was incomplete".to_owned()
        })
    );
    assert_eq!(report.first_bad, commit("dddd000"));
    assert_eq!(report.last_good, commit("aaaa000"));
}

#[test]
fn a_failure_that_a_later_landing_repaired_reports_nothing() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let broken = commit("bbbb000");
    world.work.add_landing("tk.1", &["bbbb000"], &["src/a"]);
    world.work.set_checks(&broken, vec![check("a", 'a')]);
    world
        .work
        .set_outcome(&broken, &name("unit:a"), Outcome::Fail);
    // The repair landed while `broken` was being verified.
    let repaired = commit("cccc000");
    world.land(&repaired);
    world.git.set_changed(&broken, &repaired, &["src/a/lib.rs"]);
    world.work.set_checks(&repaired, vec![check("a", 'd')]);

    let mut verifier = world.verifier();
    assert!(matches!(
        verifier.verify(&ctx, &broken).unwrap(),
        TipOutcome::Failed { .. }
    ));
    assert!(world.hub.regressions().is_empty());
    assert_eq!(world.master(), commit("aaaa000"));

    assert_eq!(
        verifier.round(&ctx, 0).unwrap(),
        Some((repaired.clone(), TipOutcome::Verified))
    );
    assert!(world.hub.regressions().is_empty());
    assert_eq!(world.master(), repaired);
}

#[test]
fn a_failure_from_another_machine_that_passes_here_does_not_hold_master() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let flaky_elsewhere = check("a", 'a');
    world.store(&flaky_elsewhere.key, Outcome::Fail, "m-2");
    world.work.set_checks(&tip, vec![flaky_elsewhere.clone()]);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    let runs = world.work.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].2, vec![flaky_elsewhere.check]);
    assert!(
        runs[0].3,
        "the foreign failure runs again here, ignoring the cache"
    );
    assert!(world.hub.regressions().is_empty());
}

#[test]
fn a_check_only_another_platform_runs_holds_master_until_it_passes_there() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let here = check("a", 'a');
    let mut elsewhere = check("a", 'e');
    elsewhere.runnable = false;
    world.work.set_checks(&tip, vec![here, elsewhere.clone()]);

    let mut verifier = world.verifier();
    assert_eq!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Waiting {
            checks: vec![name("unit:a")],
            unrunnable: Vec::new(),
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));

    world.store(&elsewhere.key, Outcome::Pass, "m-2");
    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);
    assert_eq!(world.master(), tip);
}

/// Two landings by tickets and one commit pushed by hand, recorded at the hub as `ignite land`
/// records them: `tk.1` landed `bbbb000` and `bbbb111`, `tk.2` landed `cccc000`, and `dddd000`
/// has no ticket. Each commit changes the paths given.
fn landed_world(paths: [&[&str]; 4]) -> World {
    let world = World::new();
    world.record_landing("tk.1", &["bbbb000", "bbbb111"]);
    world.record_landing("tk.2", &["cccc000"]);
    for (sha, paths) in ["bbbb000", "bbbb111", "cccc000", "dddd000"]
        .iter()
        .zip(paths)
    {
        world.work.add_commit(sha, paths);
    }
    world
}

#[test]
fn a_regression_names_the_ticket_whose_landing_introduced_it() {
    // tk.1's first commit touched the check's inputs; its second did not.
    let world = landed_world([&["src/a/lib.rs"], &["docs/x"], &["docs/y"], &["docs/z"]]);
    let ctx = world.ctx("m-1");
    let tip = commit("dddd000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);

    world.verifier().verify(&ctx, &tip).unwrap();

    let report = &world.hub.regressions()[0];
    assert_eq!(report.caused_by, Some(ticket("tk.1")));
    assert!(report.suspects.is_empty());
    // The whole landing is blamed, so the first bad commit is its last.
    assert_eq!(report.first_bad, commit("bbbb111"));
    assert_eq!(report.last_good, commit("aaaa000"));
    assert_eq!(world.work.runs().len(), 1);
}

#[test]
fn a_range_lists_every_ticket_in_it() {
    let world = landed_world([&["src/a"], &["src/a"], &["src/a"], &["src/a"]]);
    let ctx = world.ctx("m-1");
    let tip = commit("dddd000");
    world.land(&tip);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    // The first run of the search says nothing, so it ends with the whole range.
    world
        .work
        .set_outcome(&commit("bbbb111"), &failing.check, Outcome::Incomplete);

    world.verifier().verify(&ctx, &tip).unwrap();

    let report = &world.hub.regressions()[0];
    assert_eq!(report.caused_by, None);
    // The commit pushed by hand is a suspect too, but has no ticket to name.
    assert_eq!(report.suspects, vec![ticket("tk.1"), ticket("tk.2")]);
    assert_eq!(report.first_bad, tip);
}

#[test]
fn an_hour_waiting_on_another_platform_reports_it_once_and_master_waits() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let here = check("a", 'a');
    let mut elsewhere = check("w", 'e');
    elsewhere.runnable = false;
    elsewhere.key.platform = Some(Os::Windows);
    let mut verifier = world.verifier();
    let verify_at = |sha: &str, verifier: &mut TipVerifier| {
        let tip = commit(sha);
        world.land(&tip);
        world
            .work
            .set_checks(&tip, vec![here.clone(), elsewhere.clone()]);
        verifier.verify(&ctx, &tip).unwrap()
    };

    assert!(matches!(
        verify_at("bbbb000", &mut verifier),
        TipOutcome::Waiting { .. }
    ));
    // A newer tip does not restart the wait.
    world.clock.advance(MACHINE_WAIT_S - 1.0);
    verify_at("cccc000", &mut verifier);
    assert!(world.hub.tips_waiting().is_empty());

    world.clock.advance(1.0);
    verify_at("cccc000", &mut verifier);
    world.clock.advance(600.0);
    verify_at("cccc000", &mut verifier);

    let reports = world.hub.tips_waiting();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].commit, commit("cccc000"));
    assert_eq!(reports[0].platform, Os::Windows);
    assert_eq!(reports[0].checks, vec![name("unit:w")]);
    assert!((reports[0].waited_s - MACHINE_WAIT_S).abs() < f64::EPSILON);
    assert_eq!(world.master(), commit("aaaa000"));
    assert!(world.git.pushes().is_empty());
}

#[test]
fn a_failure_only_another_platform_can_run_holds_master_and_is_not_searched_here() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("dddd000");
    world.land(&tip);
    world
        .work
        .add_landing("tk.1", &["bbbb000"], &["src/a/one.rs"]);
    world
        .work
        .add_landing("tk.2", &["cccc000"], &["src/a/two.rs"]);
    world
        .work
        .add_landing("tk.3", &["dddd000"], &["src/a/three.rs"]);
    let mut elsewhere = check("a", 'e');
    elsewhere.runnable = false;
    world.work.set_checks(&tip, vec![elsewhere.clone()]);
    world.store(&elsewhere.key, Outcome::Fail, "m-2");

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:a")]
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));
    // This machine's version of the check would pass everywhere and blame tk.3.
    assert!(world.work.runs().is_empty());
    assert!(world.hub.regressions().is_empty());
}

#[test]
fn a_failure_with_no_landing_between_the_commits_still_reports_the_range() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("cccc000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert!(matches!(outcome, TipOutcome::Failed { .. }));
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].first_bad, tip);
    assert_eq!(reports[0].last_good, commit("aaaa000"));
    assert_eq!(reports[0].caused_by, None);
    assert!(reports[0].suspects.is_empty());
    assert!(matches!(
        reports[0].search,
        Some(CulpritSearch::NotRun { .. })
    ));
}

fn scratch_file(text: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = ignite_core::test_files::temp_dir()
        .join(format!("ignite-tip-output-{}-{n}.txt", std::process::id()));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_failure_quotes_the_output_file_this_machine_wrote() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("cccc000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    let file = scratch_file("first line\nlast line\n");
    world
        .work
        .set_output(&name("unit:a"), file.to_str().unwrap());

    world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(world.hub.regressions()[0].output, "first line\nlast line");
    let _ = std::fs::remove_file(file);
}

#[test]
fn an_empty_output_file_is_replaced_by_its_path() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("cccc000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    let file = scratch_file("");
    world
        .work
        .set_output(&name("unit:a"), file.to_str().unwrap());

    world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(
        world.hub.regressions()[0].output,
        file.display().to_string()
    );
    let _ = std::fs::remove_file(file);
}

#[test]
fn an_output_path_in_a_cached_result_is_printed_and_not_opened() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("cccc000");
    world.land(&tip);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    // A cached failure names a path. On the machine that reads it, that path can be any file.
    let private = scratch_file("contents of a file on this machine");
    world
        .hub
        .store(&[NewResult {
            key: failing.key.clone(),
            outcome: Outcome::Fail,
            machine: MachineId::new("m-1").unwrap(),
            record: RecordId(1),
            output: Some(private.display().to_string()),
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        }])
        .unwrap();

    // The failure was already run again here once, so it stands.
    let mut verifier = world.verifier();
    verifier.rechecked.insert(failing.key.clone());
    verifier.verify(&ctx, &tip).unwrap();

    assert_eq!(
        world.hub.regressions()[0].output,
        private.display().to_string()
    );
    let _ = std::fs::remove_file(private);
}

mod budget;
mod culprit;
mod green;
mod memory;
mod more;
mod process;
mod products;
mod rounds;
mod speed;
