//! Refs outside `refs/heads/`, such as `refs/ignite/staging`: Ignite keeps its integration refs
//! and saved work there so the remote lists them as no branch. Every git operation that takes a
//! branch name takes such a ref the same way.

mod support;

use ignite_core::traits::{Error, Git, Push};
use ignite_git::fastforward::{self, Branch, FastForward, Step};
use ignite_git::{GitCli, push, refs, save};
use support::{Scratch, commit_all, git, remote_and_clone, write};

const STAGING: &str = "refs/ignite/staging";

#[test]
fn fetching_a_ref_does_not_run_automatic_maintenance() {
    let scratch = Scratch::new("fetch-maintenance");
    let (_, clone) = remote_and_clone(&scratch);
    let expected = refs::head(&clone).unwrap();
    git(&clone, &["config", "maintenance.auto", "true"]);
    git(&clone, &["config", "maintenance.autoDetach", "false"]);
    git(&clone, &["config", "maintenance.gc.enabled", "false"]);
    git(
        &clone,
        &["config", "maintenance.commit-graph.enabled", "true"],
    );
    git(&clone, &["config", "maintenance.commit-graph.auto", "-1"]);
    let commit_graph = clone.join(".git/objects/info/commit-graphs/commit-graph-chain");

    assert_eq!(
        refs::remote_tip(&clone, "origin", "main").unwrap(),
        expected
    );
    assert!(!commit_graph.exists(), "fetch started maintenance");

    git(&clone, &["maintenance", "run", "--auto"]);
    assert!(
        commit_graph.is_file(),
        "maintenance did not write the graph"
    );
}

#[test]
fn a_custom_ref_is_pushed_replaced_fetched_saved_and_deleted_and_no_branch_appears() {
    let scratch = Scratch::new("custom-refs");
    let (remote, clone) = remote_and_clone(&scratch);
    let cli = GitCli {
        repo: clone.clone(),
        remote: "origin".to_owned(),
    };
    let start = cli.tip("main").unwrap();
    assert!(matches!(cli.tip(STAGING), Err(Error::NotFound(_))));

    assert_eq!(
        cli.push(&clone, &start, STAGING).unwrap(),
        Push::Pushed {
            commit: start.clone()
        }
    );
    assert_eq!(cli.tip(STAGING).unwrap(), start);
    // The fetch keeps it at `refs/remotes/origin/ignite/staging`, which `origin/ignite/staging`
    // names on the command line.
    assert_eq!(cli.known_tip(STAGING).unwrap(), start);
    assert_eq!(
        git(&clone, &["rev-parse", "origin/ignite/staging"]).trim(),
        start.as_str()
    );

    write(&clone, "b.txt", "two\n");
    let second = commit_all(&clone, "second");
    cli.push(&clone, &second, STAGING).unwrap();
    // A push that does not fast-forward is rejected, as for a branch.
    git(
        &clone,
        &["checkout", "--quiet", "-b", "side", start.as_str()],
    );
    write(&clone, "c.txt", "side\n");
    let side = commit_all(&clone, "side");
    assert!(matches!(
        cli.push(&clone, &side, STAGING).unwrap(),
        Push::Rejected { .. }
    ));
    // A lease on the commit last seen replaces it; a stale lease is rejected.
    assert!(matches!(
        cli.replace(&clone, &side, STAGING, &start).unwrap(),
        Push::Rejected { .. }
    ));
    assert_eq!(
        cli.replace(&clone, &side, STAGING, &second).unwrap(),
        Push::Pushed {
            commit: side.clone()
        }
    );
    assert_eq!(refs::remote_tip(&clone, "origin", STAGING).unwrap(), side);

    let saved = save::save(&clone, "origin", "refs/ignite/tickets/T1", "saved").unwrap();
    assert_eq!(saved.commit, side);
    assert_eq!(cli.tip("refs/ignite/tickets/T1").unwrap(), side);

    // The remote lists `main` as its only branch.
    assert_eq!(
        git(
            &remote,
            &["for-each-ref", "--format=%(refname)", "refs/heads"]
        )
        .trim(),
        "refs/heads/main"
    );

    push::delete(&clone, "origin", "refs/ignite/tickets/T1").unwrap();
    push::delete(&clone, "origin", "refs/ignite/tickets/T1").unwrap();
    assert!(matches!(
        cli.tip("refs/ignite/tickets/T1"),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn the_main_checkout_keeps_its_local_branch_at_a_custom_ref() {
    let scratch = Scratch::new("custom-refs-ff");
    let (_, clone) = remote_and_clone(&scratch);
    let start = refs::head(&clone).unwrap();
    git(&clone, &["branch", "staging", start.as_str()]);
    // Another clone moves the remote's `refs/ignite/staging` ahead.
    git(&clone, &["checkout", "--quiet", "--detach"]);
    write(&clone, "b.txt", "two\n");
    let ahead = commit_all(&clone, "ahead");
    git(
        &clone,
        &["push", "--quiet", "origin", &format!("{ahead}:{STAGING}")],
    );
    git(&clone, &["checkout", "--quiet", "staging"]);

    let mut expected = [
        Step::CheckingCheckout,
        Step::ReadingHead,
        Step::Fetching(STAGING),
        Step::Comparing("refs/heads/staging"),
        Step::Merging("refs/heads/staging"),
    ]
    .into_iter();
    let done =
        fastforward::main_checkout_with_progress(&clone, "origin", &[STAGING.to_owned()], |step| {
            assert_eq!(Some(step), expected.next());
        })
        .unwrap();
    assert_eq!(expected.next(), None);
    assert_eq!(
        done,
        FastForward::Branches(vec![(
            STAGING.to_owned(),
            Branch::Moved {
                from: start,
                to: ahead.clone()
            }
        )])
    );
    assert_eq!(refs::head(&clone).unwrap(), ahead);
    assert_eq!(refs::branch(&clone).unwrap().as_deref(), Some("staging"));
    // A custom ref outside `refs/heads/` leaves upstream config alone.
    assert_eq!(refs::branch_upstream(&clone, "staging").unwrap(), None);
}

#[test]
fn main_checkout_repairs_branch_upstream_and_leaves_other_remotes_alone() {
    let scratch = Scratch::new("branch-upstream-sync");
    let (_, clone) = remote_and_clone(&scratch);
    let start = refs::head(&clone).unwrap();
    git(&clone, &["branch", "staging", start.as_str()]);
    git(
        &clone,
        &["push", "--quiet", "origin", "staging:refs/heads/staging"],
    );
    // Simulate a checkout left over from when staging lived under `refs/ignite/staging`.
    git(&clone, &["config", "branch.staging.remote", "origin"]);
    git(
        &clone,
        &["config", "branch.staging.merge", "refs/ignite/staging"],
    );

    let done = fastforward::main_checkout(&clone, "origin", &["staging".to_owned()]).unwrap();
    assert_eq!(
        done,
        FastForward::Branches(vec![("staging".to_owned(), Branch::Current)])
    );
    assert_eq!(
        refs::branch_upstream(&clone, "staging").unwrap(),
        Some(("origin".to_owned(), "refs/heads/staging".to_owned()))
    );

    // A branch explicitly tracking another remote is not overwritten.
    git(&clone, &["config", "branch.staging.remote", "upstream"]);
    git(
        &clone,
        &["config", "branch.staging.merge", "refs/heads/other"],
    );
    fastforward::main_checkout(&clone, "origin", &["staging".to_owned()]).unwrap();
    assert_eq!(
        refs::branch_upstream(&clone, "staging").unwrap(),
        Some(("upstream".to_owned(), "refs/heads/other".to_owned()))
    );
}
