//! An idle job that fails waits longer before each new attempt, keeps waiting after a restart,
//! and is reported to the hub once after a few failures.

use std::sync::atomic::Ordering;

use ignite_core::protocol::Usage;

use super::{Fixture, IdleJob};
use crate::supervise::ResultFile;

const MINUTE: f64 = 60.0;

fn rebuild() -> IdleJob {
    IdleJob::Rebuild {
        name: "core-body".to_owned(),
    }
}

#[test]
fn a_failing_rebuild_backs_off_and_is_reported_once() {
    let f = Fixture::new();
    f.compiler.broken.store(true, Ordering::SeqCst);
    let idle = f.idle();

    // The first failure: half an hour's wait.
    assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));
    let error = idle.run(&f.ctx, &rebuild()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("failed to build before the edit")
    );
    assert_ne!(idle.next_job(&f.ctx), Some(rebuild()));
    f.clock.advance(29.0 * MINUTE);
    assert_ne!(idle.next_job(&f.ctx), Some(rebuild()));

    // The wait is in the state file, so a restarted daemon keeps it.
    f.clock.advance(2.0 * MINUTE);
    let idle = f.idle();
    assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));

    // The second failure doubles the wait to an hour.
    idle.run(&f.ctx, &rebuild()).unwrap_err();
    f.clock.advance(59.0 * MINUTE);
    assert_ne!(idle.next_job(&f.ctx), Some(rebuild()));
    f.clock.advance(2.0 * MINUTE);
    assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));
    assert!(f.hub.justiciar_reports().is_empty());

    // The third failure is reported.
    idle.run(&f.ctx, &rebuild()).unwrap_err();
    let reports = f.hub.justiciar_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].subject, "idle:rebuild:core-body");
    assert!(reports[0].title.contains("rebuild core-body"));
    assert!(
        reports[0]
            .details
            .contains("failed to build before the edit"),
        "{}",
        reports[0].details
    );

    // Later failures wait longer and report nothing more.
    f.clock.advance(119.0 * MINUTE);
    assert_ne!(idle.next_job(&f.ctx), Some(rebuild()));
    f.clock.advance(2.0 * MINUTE);
    assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));
    idle.run(&f.ctx, &rebuild()).unwrap_err();
    assert_eq!(f.hub.justiciar_reports().len(), 1);

    // A success clears the failures: the edit is then due by commits again, as usual.
    f.compiler.broken.store(false, Ordering::SeqCst);
    f.clock.advance(241.0 * MINUTE);
    assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));
    idle.run(&f.ctx, &rebuild()).unwrap();
    assert!(idle.state().failing.is_empty());
    assert_eq!(idle.state().counts.len(), 1);
    assert_ne!(idle.next_job(&f.ctx), Some(rebuild()));
}

#[test]
fn a_job_with_no_free_landing_tree_keeps_its_failures() {
    let f = Fixture::new();
    f.compiler.broken.store(true, Ordering::SeqCst);
    let idle = f.idle();
    idle.run(&f.ctx, &rebuild()).unwrap_err();

    // The retry finds no spare landing tree, so the job did not run: its failure stays.
    f.clock.advance(31.0 * MINUTE);
    f.host.busy.store(true, Ordering::SeqCst);
    idle.run(&f.ctx, &rebuild()).unwrap();
    assert_eq!(idle.state().failing["rebuild core-body"].failures, 1);
}

/// On 2026-11-19 the scheduler stopped the idle rebuild's builds three times in a row to give
/// their cores to landings. Each stopped build ended by a signal, the job counted it as a
/// failure, and the third filed a justiciar report against a job with nothing wrong with it.
#[test]
fn a_build_stopped_from_outside_is_not_a_failure() {
    let f = Fixture::new();
    f.compiler.broken.store(true, Ordering::SeqCst);
    // The failed build's run directory says a signal ended it before its hang limit.
    let ended = |exit: Option<i32>| {
        let result = ResultFile {
            exit,
            output: f.compiler.dir.join("build.log"),
            usage: Usage::default(),
            hung: false,
        };
        let json = serde_json::to_vec(&result).unwrap();
        std::fs::write(f.compiler.dir.join("result.json"), json).unwrap();
    };
    ended(None);
    let idle = f.idle();

    for _ in 0..4 {
        let error = idle.run(&f.ctx, &rebuild()).unwrap_err();
        assert!(error.to_string().contains("stopped before it finished"), "{error}");
        // The job runs again at once, with no wait and no report.
        assert_eq!(idle.next_job(&f.ctx), Some(rebuild()));
    }
    assert!(idle.state().failing.is_empty());
    assert!(f.hub.justiciar_reports().is_empty());

    // A build that exited with a code failed on its own, and counts.
    ended(Some(101));
    idle.run(&f.ctx, &rebuild()).unwrap_err();
    assert_eq!(idle.state().failing["rebuild core-body"].failures, 1);
}
