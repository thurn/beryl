//! Giving up on incomplete checks: which are held off and what the hub is told.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{CheckName, Commit, InputHash, MachineId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{IncompleteReason, ResultKey};
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::super::{ListingPhase, TipCheck, TipLanding, TipRun, TipSet, TipVerifier, TipWork};
use crate::server::Context;

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
"#;

/// Work that is never asked for anything: these tests call `note_incomplete` directly.
struct NoWork;

impl TipWork for NoWork {
    fn checks(&self, _ctx: &Context, _commit: &Commit, _phase: ListingPhase) -> Result<TipSet> {
        unreachable!("not listed in these tests")
    }
    fn run(&self, _: &Context, _: &Commit, _: &[CheckName], _: bool) -> Vec<TipRun> {
        unreachable!("not run in these tests")
    }
    fn landings(&self, _: &Context, _: &Commit, _: &Commit) -> Result<Vec<TipLanding>> {
        unreachable!("not asked in these tests")
    }
}

fn ctx(hub: &Arc<FakeHub>) -> Context {
    Context {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
        hub: hub.clone(),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn check(name: &str, hash: char) -> TipCheck {
    let check = CheckName::new(name).unwrap();
    TipCheck {
        key: ResultKey {
            check: check.clone(),
            hash: InputHash::new(hash.to_string().repeat(64)).unwrap(),
            platform: None,
        },
        check,
        family: "rust".to_owned(),
        inputs: Vec::new(),
        runnable: true,
        reuse: true,
        machine_relative: false,
        kind: crate::tip::TipCheckKind::Other,
    }
}

fn incomplete(check: &TipCheck, reason: IncompleteReason) -> TipRun {
    TipRun {
        check: check.check.clone(),
        input_hash: None,
        outcome: Outcome::Incomplete,
        output: Some("the run's output".to_owned()),
        reason: Some(reason),
        build: None,
    }
}

/// 710 Rust checks whose cargo-nextest run could not link its test binary were held off for an
/// hour, then two, and `master` waited on them although nothing about the checks had hung. A
/// check whose build failed is now told to the hub but never held off.
#[test]
fn a_check_whose_build_failed_is_reported_but_never_held_off() {
    let hub = Arc::new(FakeHub::default());
    let ctx = ctx(&hub);
    let tip = Commit::new("bbbb000").unwrap();
    let unbuilt = check("rust:session::a", 'a');
    let unreported = check("rust:session::b", 'b');
    let verdicts: HashMap<ResultKey, TipRun> = [
        (
            unbuilt.key.clone(),
            incomplete(&unbuilt, IncompleteReason::BuildFailed),
        ),
        (
            unreported.key.clone(),
            incomplete(&unreported, IncompleteReason::Unreported),
        ),
    ]
    .into_iter()
    .collect();
    let mut verifier = TipVerifier::new(Arc::new(NoWork), "test");

    verifier.note_incomplete(
        &ctx,
        &tip,
        &[unbuilt.clone(), unreported.clone()],
        &verdicts,
        true,
    );

    assert!(!verifier.held_off.contains_key(&unbuilt.key));
    assert!(!verifier.incomplete_runs.contains_key(&unbuilt.key));
    assert!(
        verifier.held_off.contains_key(&unreported.key),
        "the control"
    );
    let reasons: Vec<IncompleteReason> =
        hub.incomplete_reports().iter().map(|r| r.reason).collect();
    assert!(
        reasons.contains(&IncompleteReason::BuildFailed),
        "{reasons:?}"
    );
}
