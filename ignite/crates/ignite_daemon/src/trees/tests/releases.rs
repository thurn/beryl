//! Tests for release failures, overlapping releases, and their running verbs.

use super::*;

#[test]
fn a_failed_reset_in_a_release_leaves_the_tree_leased_for_a_retry() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let missing = Commit::new("1234567890abcdef1234567890abcdef12345678").unwrap();

    let error = f
        .trees
        .release_unsaved(&lease.tree, &missing, None)
        .unwrap_err();

    assert!(!matches!(error, Error::Conflict(_)), "{error:?}");
    assert_eq!(f.state("tree-01"), "leased");
    f.trees
        .release_unsaved(&lease.tree, &f.tip(), None)
        .unwrap();
    assert_eq!(f.state("tree-01"), "free");
}

#[test]
fn of_two_overlapping_releases_only_the_first_resets_the_tree() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let row = f.trees.row(&lease.tree).unwrap();

    // Another release has taken the tree out of `leased` since this one read its row.
    store::set_state(
        &f.trees.lock().db,
        lease.tree.as_str(),
        store::State::Resetting,
        "t",
    )
    .unwrap();
    let error = f
        .trees
        .reset_leased(&lease.tree, &row, &f.tip(), None)
        .unwrap_err();

    assert!(matches!(error, Error::Conflict(_)), "{error:?}");
    assert_eq!(f.state("tree-01"), "resetting");
}

#[test]
fn a_tree_held_for_a_waiter_that_never_came_is_freed_for_the_next_lease() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    assert!(matches!(
        f.trees.lease(f.request("T.2", false)).unwrap(),
        Lease::Waiting(_)
    ));

    f.trees.abandon(&lease);

    assert_eq!(f.state("tree-01"), "free");
    f.lease("T.3", false);
}

/// A verb running from `tree`, as the server lists it (see `crate::steps`).
fn running_verb(verb: &str, tree: &str) -> Arc<crate::steps::Activity> {
    crate::steps::global().begin(
        crate::steps::Kind::Verb(verb.to_owned()),
        crate::steps::Work {
            tree: Some(id(tree)),
            ..crate::steps::Work::default()
        },
    )
}

#[test]
fn closing_a_ticket_stops_its_running_landing_before_the_tree_is_freed() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let landing = running_verb("land", "tree-01");
    let (trees, seen) = (f.trees.clone(), landing.clone());
    let unwinding = std::thread::spawn(move || {
        assert!(seen.wait_stop_requested(std::time::Duration::from_secs(60)));
        let state = trees
            .status()
            .unwrap()
            .trees
            .into_iter()
            .find(|t| t.tree.as_str() == "tree-01")
            .unwrap()
            .state;
        seen.finish();
        state
    });

    f.trees
        .release_unsaved(&lease.tree, &f.tip(), None)
        .unwrap();

    assert_eq!(
        unwinding.join().unwrap(),
        "resetting",
        "stopped before it was freed"
    );
    assert!(landing.is_finished());
    assert_eq!(f.state("tree-01"), "free");
}

#[test]
fn a_landing_whose_push_began_keeps_its_tree_leased_and_the_release_says_why() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let landing = running_verb("land", "tree-01");
    landing.attach_handle(&ignite_core::ids::Handle::new("L-2").unwrap());
    assert!(landing.refuse_stop("its push to staging has begun"));

    let error = f
        .trees
        .release_unsaved(&lease.tree, &f.tip(), None)
        .unwrap_err();

    assert_eq!(
        error,
        Error::Conflict(
            "tree-01 is not freed: `ignite land` (L-2) cannot be stopped: its push to staging \
             has begun"
                .to_owned()
        )
    );
    assert_eq!(f.state("tree-01"), "leased");
    landing.finish();
    f.trees
        .release_unsaved(&lease.tree, &f.tip(), None)
        .unwrap();
    assert_eq!(f.state("tree-01"), "free");
}

#[test]
fn a_verb_in_another_daemons_tree_of_the_same_name_is_left_running() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let elsewhere = crate::steps::global().begin(
        crate::steps::Kind::Verb("test".to_owned()),
        crate::steps::Work {
            tree: Some(id("tree-01")),
            tree_path: Some(PathBuf::from("/another/daemon/trees/tree-01")),
            ..crate::steps::Work::default()
        },
    );
    f.trees
        .release_unsaved(&lease.tree, &f.tip(), None)
        .unwrap();
    assert!(!elsewhere.stop_requested());
}

#[test]
fn a_lease_frees_the_ticket_branch_from_a_tree_a_failed_release_left_on_it() {
    let f = Fixture::new(2, 1);
    let first = f.lease("T.1", false);
    // The release's reset failed, here on a lock a killed command left, so the first tree is
    // still on the ticket's branch when the hub gives the ticket to another session.
    let git_dir = git(&first.path, &["rev-parse", "--absolute-git-dir"]);
    fs::write(Path::new(git_dir.trim()).join("index.lock"), "").unwrap();
    let mut request = f.request("T.1", false);
    request.session = Some(SessionId::new("local_next").unwrap());
    let Lease::Ready(second) = f.trees.lease(request).unwrap() else {
        panic!("expected a free tree");
    };

    assert_ne!(second.tree, first.tree);
    assert_eq!(
        git(&second.path, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "work/T.1"
    );
    assert_eq!(
        git(&first.path, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "HEAD",
        "the first tree is detached"
    );
}

#[test]
fn a_failed_release_does_not_restore_a_tree_taken_for_a_sweep() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let missing = Commit::new("1234567890abcdef1234567890abcdef12345678").unwrap();
    let verb = running_verb("test", "tree-01");
    let (trees, tree) = (f.trees.clone(), lease.tree.clone());

    let release = std::thread::spawn(move || trees.release_unsaved(&tree, &missing, None));
    assert!(verb.wait_stop_requested(std::time::Duration::from_secs(60)));
    assert_eq!(f.state("tree-01"), "resetting");

    // While the release waits in `stop_work`, another operation frees the tree and a sweep
    // takes it as `resetting` with no ticket or session.
    store::release(&f.trees.lock().db, "tree-01", None, &f.trees.now()).unwrap();
    let swept = f
        .trees
        .take_for_sweep("tree-01")
        .unwrap()
        .expect("free tree should be taken for sweep");
    verb.finish();

    let error = release.join().unwrap().unwrap_err();
    assert!(!matches!(error, Error::Conflict(_)), "{error:?}");
    let row = f.trees.row(&lease.tree).unwrap();
    assert_eq!(
        row.state, "resetting",
        "the failed release must not turn the swept row into an unheld leased tree"
    );
    assert_eq!(row.ticket, None);
    assert_eq!(row.session, None);

    f.trees.put_back(&swept).unwrap();
    assert_eq!(f.state("tree-01"), "free");
}

#[test]
fn free_unheld_leases_frees_only_holderless_leased_ticket_trees() {
    let f = Fixture::new(2, 1);
    let held = f.lease("T.1", false);
    let mut leaked = f.trees.row(&id("tree-02")).unwrap();
    leaked.state = store::State::Leased.as_str().to_owned();
    leaked.ticket = None;
    leaked.session = None;
    leaked.leased_at = Some(f.trees.now());
    store::upsert(&f.trees.lock().db, &leaked).unwrap();

    let freed = f.trees.free_unheld_leases().unwrap();
    assert_eq!(freed, vec![id("tree-02")]);
    assert_eq!(f.state("tree-02"), "free");
    assert_eq!(f.trees.row(&id("tree-02")).unwrap().leased_at, None);
    assert_eq!(f.state(held.tree.as_str()), "leased");
}
