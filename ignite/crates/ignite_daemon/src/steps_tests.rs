use std::sync::Arc;
use std::time::Duration;

use ignite_core::fakes::FakeClock;
use ignite_core::ids::{Handle, TicketId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;

use super::*;

fn registry() -> (Arc<FakeClock>, Steps) {
    let clock = Arc::new(FakeClock::new(Timestamp::from_millis(1_000_000)));
    let steps = Steps::new(clock.clone());
    (clock, steps)
}

#[test]
fn deep_code_reaches_the_activity_entered_on_its_thread() {
    let (clock, steps) = registry();
    let activity = steps.begin(
        Kind::Verb("land".to_owned()),
        Work {
            class: Some(Class::Land),
            ..Work::default()
        },
    );
    step("before anything is entered");
    assert_eq!(class(), None);
    {
        let _entered = activity.enter();
        step("rebasing");
        clock.advance(30.0);
        step("building rust-tests in landing-3");
        clock.advance(90.0);
        let _wait = waiting("8 cores");
        clock.advance(60.0);
        assert_eq!(class(), Some(Class::Land));
        let view = activity.view();
        assert_eq!(
            view.step.as_deref(),
            Some("building rust-tests in landing-3")
        );
        assert_eq!(
            view.doing(),
            "building rust-tests in landing-3 (2 min), waiting for 8 cores (60 s)"
        );
        assert!((view.age_s - 180.0).abs() < 1e-9, "{}", view.age_s);
    }
    assert_eq!(activity.view().waiting_for, None);
    assert!(current().is_none());
}

#[test]
fn a_thread_spawned_for_the_work_can_enter_it() {
    let (_, steps) = registry();
    let activity = steps.begin(Kind::Loop("ignite-tip".to_owned()), Work::default());
    let _entered = activity.enter();
    let scope = current();
    std::thread::spawn(move || {
        let _entered = scope.as_ref().map(Activity::enter);
        set_class(Class::Tip);
        step("listing family rust");
    })
    .join()
    .unwrap();
    assert_eq!(activity.view().step.as_deref(), Some("listing family rust"));
    assert_eq!(class(), Some(Class::Tip));
}

#[test]
fn a_stop_is_refused_once_the_work_says_it_may_not_be_interrupted() {
    let (_, steps) = registry();
    let landing = steps.begin(Kind::Verb("land".to_owned()), Work::default());
    let _entered = landing.enter();
    assert!(refuse_stop("its push to staging has begun"));
    assert_eq!(
        landing.request_stop(),
        Err("its push to staging has begun".to_owned())
    );
    assert!(!stop_requested());

    let other = steps.begin(Kind::Verb("test".to_owned()), Work::default());
    let _inner = other.enter();
    assert_eq!(other.request_stop(), Ok(()));
    assert!(stop_requested());
    assert!(!refuse_stop("too late"), "a stop already asked for wins");
}

#[test]
fn the_registry_finds_running_work_by_handle_and_tree_and_forgets_finished_work() {
    let (_, steps) = registry();
    let tree = TreeId::new("tree-04").unwrap();
    let land = steps.begin(
        Kind::Verb("land".to_owned()),
        Work {
            tree: Some(tree.clone()),
            ticket: Some(TicketId::new("Tqtj2").unwrap()),
            ..Work::default()
        },
    );
    let handle = Handle::new("L-2").unwrap();
    land.attach_handle(&handle);
    let round = steps.begin(Kind::Loop("ignite-idle".to_owned()), Work::default());
    assert_eq!(steps.live().len(), 2);
    assert!(steps.by_handle(&handle).is_some());
    let path = std::path::Path::new("/repo/trees/tree-04");
    assert_eq!(
        steps.in_tree(&tree, path).len(),
        1,
        "loops are not in a tree"
    );
    land.finish();
    assert!(land.wait_finished(Duration::ZERO));
    assert!(steps.by_handle(&handle).is_none());
    assert!(steps.in_tree(&tree, path).is_empty());
    drop(round);
    assert!(steps.live().is_empty(), "a dropped activity is not listed");
}

#[test]
fn a_subscriber_hears_each_step_and_wait_as_it_starts() {
    let (_, steps) = registry();
    let activity = steps.begin(Kind::Verb("land".to_owned()), Work::default());
    let heard = activity.subscribe();
    let _entered = activity.enter();
    step("rebasing onto staging");
    drop(waiting("the staging lease"));
    activity.finish();
    let lines: Vec<String> = heard.try_iter().collect();
    assert_eq!(
        lines,
        ["rebasing onto staging", "waiting for the staging lease"]
    );
}
