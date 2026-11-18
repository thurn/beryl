use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId, RecordId, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{NewResult, ResultKey, TipClaim};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Git, Hub, Result};

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
cap = 400
[machine.local]
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

/// Checks, run outcomes and landings scripted by commit. A run stores its results at the hub
/// under the keys the commit's checks have there, as the real runner does.
/// One run: the machine, the commit, the checks named and whether the cache was ignored.
type Run = (MachineId, Commit, Vec<CheckName>, bool);

#[derive(Default)]
struct FakeWork {
    checks: Mutex<HashMap<Commit, Vec<TipCheck>>>,
    outcomes: Mutex<HashMap<(Commit, CheckName), Outcome>>,
    landings: Mutex<Vec<TipLanding>>,
    runs: Mutex<Vec<Run>>,
}

impl FakeWork {
    fn set_checks(&self, at: &Commit, checks: Vec<TipCheck>) {
        self.checks.lock().unwrap().insert(at.clone(), checks);
    }

    fn set_outcome(&self, at: &Commit, check: &CheckName, outcome: Outcome) {
        self.outcomes
            .lock()
            .unwrap()
            .insert((at.clone(), check.clone()), outcome);
    }

    fn add_landing(&self, ticket_id: &str, commits: &[&str], paths: &[&str]) {
        self.landings.lock().unwrap().push(TipLanding {
            ticket: Some(ticket(ticket_id)),
            commits: commits.iter().map(|c| commit(c)).collect(),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        });
    }

    fn runs(&self) -> Vec<Run> {
        self.runs.lock().unwrap().clone()
    }
}

impl TipWork for FakeWork {
    fn checks(&self, _ctx: &Context, at: &Commit) -> Result<Vec<TipCheck>> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .get(at)
            .cloned()
            .unwrap_or_default())
    }

    fn run(&self, ctx: &Context, at: &Commit, checks: &[CheckName], rerun: bool) -> Vec<TipRun> {
        self.runs
            .lock()
            .unwrap()
            .push((ctx.machine.clone(), at.clone(), checks.to_vec(), rerun));
        let listed = self.checks(ctx, at).unwrap();
        let outcomes = self.outcomes.lock().unwrap();
        let mut runs = Vec::new();
        for check in checks {
            let outcome = outcomes
                .get(&(at.clone(), check.clone()))
                .copied()
                .unwrap_or(Outcome::Pass);
            if let Some(listed) = listed.iter().find(|c| c.check == *check)
                && matches!(outcome, Outcome::Pass | Outcome::Fail)
            {
                let output = (outcome == Outcome::Fail).then(|| format!("{check} failed"));
                ctx.hub
                    .store(&[NewResult {
                        key: listed.key.clone(),
                        outcome,
                        machine: ctx.machine.clone(),
                        record: RecordId(1),
                        output,
                    }])
                    .unwrap();
            }
            runs.push(TipRun {
                check: check.clone(),
                outcome,
                output: (outcome == Outcome::Fail).then(|| format!("{check} failed")),
            });
        }
        runs
    }

    fn landings(&self, _ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        // The scripted landings all lie between the commits a test passes.
        assert_ne!(from, to);
        Ok(self.landings.lock().unwrap().clone())
    }
}

struct World {
    hub: Arc<FakeHub>,
    git: Arc<FakeGit>,
    work: Arc<FakeWork>,
}

impl World {
    fn new() -> World {
        let world = World {
            hub: Arc::new(FakeHub::default()),
            git: Arc::new(FakeGit::default()),
            work: Arc::new(FakeWork::default()),
        };
        world.git.set_tip("master", commit("aaaa000"));
        world
    }

    fn ctx(&self, machine: &str) -> Context {
        Context {
            config: Config::parse("ignite.toml", CONFIG).unwrap(),
            repo: PathBuf::from("/repo"),
            machine: MachineId::new(machine).unwrap(),
            clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
            hub: self.hub.clone(),
            git: self.git.clone(),
            launcher: Arc::new(FakeLauncher::default()),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        }
    }

    fn verifier(&self) -> TipVerifier {
        TipVerifier::new(self.work.clone(), "test")
    }

    fn store(&self, key: &ResultKey, outcome: Outcome, machine: &str) {
        self.hub
            .store(&[NewResult {
                key: key.clone(),
                outcome,
                machine: MachineId::new(machine).unwrap(),
                record: RecordId(1),
                output: None,
            }])
            .unwrap();
    }

    fn master(&self) -> Commit {
        self.git.tip("master").unwrap()
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
    let checks: Vec<TipCheck> = (0..BATCH + 10)
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
    let TipOutcome::Waiting { checks: waiting } = one else {
        panic!("expected m-1 to wait, got {one:?}");
    };
    assert_eq!(waiting.len(), BATCH);
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
    assert_eq!(by_two.len(), BATCH);
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
            checks: vec![name("unit:a")]
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));

    world.store(&elsewhere.key, Outcome::Pass, "m-2");
    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);
    assert_eq!(world.master(), tip);
}

#[test]
fn a_newer_tip_supersedes_the_round_between_batches() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let mut first = check("a", 'a');
    first.family = "alpha".to_owned();
    let mut second = check("b", 'b');
    second.family = "beta".to_owned();
    world.work.set_checks(&tip, vec![first, second]);
    let newer = commit("cccc000");
    world.land(&newer);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Superseded { by: newer });
    assert_eq!(world.work.runs().len(), 1);
    assert_eq!(world.master(), commit("aaaa000"));
}
