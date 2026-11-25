//! Ownership of a build the daemon runs for a separate verb process.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::build::{DiskRoom, tests::fixture};

/// A disk with too little room to run a build until the test makes space.
#[derive(Default)]
struct Disk(AtomicBool);

impl DiskRoom for Disk {
    fn available(&self, _: &Path) -> Option<u64> {
        Some(if self.0.load(Ordering::Relaxed) {
            u64::MAX
        } else {
            0
        })
    }

    fn pause(&self, _: Duration) {
        std::thread::yield_now();
    }
}

#[test]
fn a_finished_verb_releases_its_build_reservation_without_starting_a_command() {
    let db = database();
    let runs = runs_folder("finished-build-owner");
    let owner = running(&db, "L-6");
    let waiter = running(&db, "L-7");
    let disk = Arc::new(Disk::default());
    let manager = Arc::into_inner(fixture("finished-build-owner", false).manager)
        .unwrap()
        .with_room(disk.clone());
    let steps = Arc::new(Steps::new(Arc::new(FakeClock::new(T0))));
    let host = Host::with_timing(
        HostParts {
            headless_dispatch: None,
            starter: Starter::new(&runs, None),
            builds: Arc::new(manager),
            landing: Arc::new(Landing::default()),
            db: db.clone(),
            steps: steps.clone(),
            clock: Arc::new(FakeClock::new(T0)),
        },
        timing(),
    );
    let mut registry = Registry::default();
    host.register(&mut registry);

    let build = |handle: &Handle, product: &str| Build {
        handle: handle.clone(),
        request_id: RequestId::new(handle.clone(), 1),
        first_asked_at: T0,
        tree: TreeId::new("tree-01").unwrap(),
        product: product.to_owned(),
        packages: vec![],
        who: BuildWho {
            class: Class::Land,
            ticket: None,
            session: None,
            retry_failure: false,
            reserve: None,
        },
    };
    let wait_for = |handle: &Handle, call: &Build, prefix: &str| {
        send(&registry, &hello(handle)).unwrap();
        let activity = steps.by_handle(handle).unwrap();
        let progress = activity.subscribe();
        assert!(matches!(
            send(&registry, call).unwrap(),
            BuildReply::Waiting { .. }
        ));
        loop {
            let line = progress.recv().unwrap();
            if line.starts_with(prefix) {
                break;
            }
        }
        activity
    };
    let first = build(&owner, "tests");
    let second = build(&waiter, "lint");
    wait_for(&owner, &first, "waiting for disk space");
    let waiting = wait_for(
        &waiter,
        &second,
        "waiting for the build of tests in tree-01 (ignite land L-6)",
    );

    // The process writes a terminal result without asking the daemon to stop first.
    // The other build should move on to its own disk wait before any space becomes available.
    let progress = waiting.subscribe();
    finish_row(&db, &owner, "done", &Reply::text(Exit::Success, "done"));
    send(&registry, &Finished { handle: owner }).unwrap();
    loop {
        let line = progress.recv().unwrap();
        if line.starts_with("waiting for disk space") {
            break;
        }
    }
    let result = |call: &Build| loop {
        match send(&registry, call).unwrap() {
            BuildReply::Waiting { .. } => {}
            reply => break reply,
        }
    };
    let BuildReply::Failed { output, .. } = result(&first) else {
        panic!("an abandoned build reported success");
    };
    assert!(
        fs::read_to_string(output)
            .unwrap()
            .contains("stopped waiting for disk space")
    );
    disk.0.store(true, Ordering::Relaxed);
    assert!(matches!(result(&second), BuildReply::Built { .. }));
}
