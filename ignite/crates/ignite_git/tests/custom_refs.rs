//! Refs outside `refs/heads/`, such as `refs/ignite/staging`: Ignite keeps its integration refs
//! and saved work there so the remote lists them as no branch. Every git operation that takes a
//! branch name takes such a ref the same way.

mod support;

use ignite_core::traits::{Error, Git, Push};
use ignite_git::fastforward::{self, Branch, FastForward};
use ignite_git::{GitCli, push, refs, save};
use support::{Scratch, commit_all, git, remote_and_clone, write};

const STAGING: &str = "refs/ignite/staging";

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

    let done = fastforward::main_checkout(&clone, "origin", &[STAGING.to_owned()]).unwrap();
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
}
