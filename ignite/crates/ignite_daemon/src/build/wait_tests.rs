//! Build wait status and cancellation with builds held open by the test launcher.

use ignite_core::fakes::FakeClock;
use ignite_core::ids::Handle;
use ignite_core::time::Timestamp;

use super::*;
use crate::steps::{Kind, Steps, Work};

#[test]
fn a_queued_build_reports_the_holder_and_keeps_the_wait_age_when_it_changes() {
    let f = fixture("wait-holder", true);
    let mut manifest = (*f.manager.settings.manifest.get()).clone();
    manifest.products.get_mut("fixed").unwrap().outputs = names(&["other-target"]);
    manifest.products.get_mut("lint").unwrap().outputs = names(&["target", "other-target"]);
    f.manager.settings.manifest.set(manifest);
    let clock = Arc::new(FakeClock::new(Timestamp::from_millis(0)));
    let steps = Steps::new(clock.clone());

    let first = {
        let work = steps.begin(Kind::Verb("check".to_owned()), Work::default());
        work.attach_handle(&Handle::new("C-1").unwrap());
        let manager = f.manager.clone();
        thread::spawn(move || {
            let _entered = work.enter();
            manager.ensure(&tree(), "fixed", &[])
        })
    };
    f.launcher.wait_for(|s| s.active == 1);
    let second = {
        let work = steps.begin(
            Kind::Loop("ignite-lease-prebuild".to_owned()),
            Work::default(),
        );
        let manager = f.manager.clone();
        thread::spawn(move || {
            let _entered = work.enter();
            manager.ensure(&tree(), "tests", &[])
        })
    };
    f.launcher.wait_for(|s| s.active == 2);

    let gate = steps.begin(Kind::Verb("check".to_owned()), Work::default());
    let progress = gate.subscribe();
    let queued = {
        let (manager, gate) = (f.manager.clone(), gate.clone());
        thread::spawn(move || {
            let _entered = gate.enter();
            manager.ensure(&tree(), "lint", &[])
        })
    };
    let next_wait = || loop {
        let line = progress.recv().unwrap();
        if line.starts_with("waiting for ") {
            break line;
        }
    };
    assert_eq!(
        next_wait(),
        "waiting for the build of fixed in tree-01 (ignite check C-1) to finish before building lint"
    );
    clock.advance(180.0);
    f.launcher.release_one();
    assert!(matches!(first.join().unwrap(), BuildResult::Built { .. }));
    assert_eq!(
        next_wait(),
        "waiting for the build of tests in tree-01 (ignite-lease-prebuild) to finish before building lint"
    );
    let view = gate.view();
    assert_eq!(view.waiting_s, Some(180.0));
    assert!(view.doing().contains("ignite-lease-prebuild"));
    assert_eq!(f.launcher.builds().len(), 2);

    f.launcher.release_one();
    assert!(matches!(second.join().unwrap(), BuildResult::Built { .. }));
    f.launcher.wait_for(|s| s.builds == 3);
    f.launcher.release_one();
    assert!(matches!(queued.join().unwrap(), BuildResult::Built { .. }));
    assert!(gate.view().waiting_for.is_none());
}

/// A command stopped while its build waited, joined to another command's build of the same
/// state or for another build writing the same outputs, went on waiting until that build ended.
/// Both waits now give up once the work that asked is stopped, and the build they waited on runs
/// on.
#[test]
fn a_stopped_request_stops_waiting_for_another_build() {
    let f = fixture("stopped-wait", true);
    let first = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &[]))
    };
    f.launcher.wait_for(|s| s.active == 1);
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    for (product, waiting) in [("tests", (1, 0)), ("lint", (0, 1))] {
        let check = steps.begin(
            crate::steps::Kind::Verb("check".to_owned()),
            crate::steps::Work::default(),
        );
        let waiter = {
            let (manager, check) = (Arc::clone(&f.manager), Arc::clone(&check));
            thread::spawn(move || {
                let _entered = check.enter();
                manager.ensure(&tree(), product, &[])
            })
        };
        until_waiting(&f.manager, waiting);
        check.request_stop().unwrap();
        let result = waiter.join().unwrap();
        let BuildResult::Failed { output, .. } = result else {
            panic!("{product}: {result:?}");
        };
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("its work was asked to stop"), "{text}");
        assert_eq!(f.manager.waiting(), (0, 0));
    }
    // The build they waited on is still the only one, and finishes as before.
    assert_eq!(f.launcher.with(|s| s.active), 1);
    f.launcher.release_one();
    assert!(matches!(first.join().unwrap(), BuildResult::Built { .. }));
    assert_eq!(f.launcher.builds().len(), 1);
}

/// While the tree's lease copies another tree's build into it, a build waits for the copy and
/// says so, and starts once the copy is done: the copy deletes and replaces the target
/// directories the build would write.
#[test]
fn a_build_waits_while_its_tree_is_seeded_and_says_so() {
    let f = fixture("seeding", false);
    let seeding = crate::trees::Seeding::start(f.dir.clone(), tree());
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    let work = steps.begin(
        crate::steps::Kind::Verb("test".to_owned()),
        crate::steps::Work::default(),
    );
    let waiter = {
        let (manager, work) = (Arc::clone(&f.manager), Arc::clone(&work));
        thread::spawn(move || {
            let _entered = work.enter();
            manager.ensure(&tree(), "tests", &[])
        })
    };
    while work.waits().is_empty() {
        assert!(!waiter.is_finished(), "{:?}", waiter.join());
        thread::yield_now();
    }
    assert_eq!(work.waits(), ["tree-01's build directories to be copied"]);
    assert!(f.launcher.builds().is_empty());

    drop(seeding);

    let result = waiter.join().unwrap();
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    assert_eq!(f.launcher.builds().len(), 1);
}
