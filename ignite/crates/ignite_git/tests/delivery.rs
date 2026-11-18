//! Rebasing, pushing, remote tips and changed paths, against temporary repositories.

mod support;

use std::fs;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Git, Push, Rebase};
use ignite_git::{GitCli, changed, refs};
use support::{Scratch, commit_all, git, head, init, remote_and_clone, write};

fn cli(repo: &std::path::Path) -> GitCli {
    GitCli {
        repo: repo.to_path_buf(),
        remote: "origin".to_owned(),
    }
}

#[test]
fn a_clean_rebase_counts_the_commits_that_landed_since() {
    let scratch = Scratch::new("rebase-clean");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "a.txt", "a\n");
    let base = commit_all(&repo, "base");
    write(&repo, "b.txt", "b\n");
    commit_all(&repo, "landed one");
    write(&repo, "c.txt", "c\n");
    let tip = commit_all(&repo, "landed two");
    git(&repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(&repo, "d.txt", "d\n");
    commit_all(&repo, "work");

    let Rebase::Clean {
        onto,
        head: new_head,
        landed_since,
    } = cli(&repo).rebase_onto(&repo, &tip).unwrap()
    else {
        panic!("expected a clean rebase");
    };
    assert_eq!((onto, landed_since), (tip.clone(), 2));
    assert_eq!(new_head, head(&repo));
    assert_eq!(
        refs::commits_between(&repo, &tip, &new_head).unwrap(),
        vec![new_head.clone()]
    );
    assert_eq!(git(&repo, &["branch", "--show-current"]).trim(), "work");
}

#[test]
fn a_conflicting_rebase_names_the_files_and_leaves_the_branch_as_it_was() {
    let scratch = Scratch::new("rebase-conflict");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "shared.txt", "base\n");
    write(&repo, "other.txt", "base\n");
    let base = commit_all(&repo, "base");
    write(&repo, "shared.txt", "tip\n");
    let tip = commit_all(&repo, "tip");
    git(&repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(&repo, "shared.txt", "work\n");
    write(&repo, "other.txt", "work\n");
    let work = commit_all(&repo, "work");

    let outcome = cli(&repo).rebase_onto(&repo, &tip).unwrap();
    assert_eq!(
        outcome,
        Rebase::Conflict {
            files: vec!["shared.txt".to_owned()]
        }
    );
    assert_eq!(head(&repo), work);
    assert_eq!(git(&repo, &["branch", "--show-current"]).trim(), "work");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert!(!repo.join(".git/rebase-merge").exists());
}

#[test]
fn a_rebase_refused_for_uncommitted_edits_is_an_error() {
    let scratch = Scratch::new("rebase-dirty");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "a.txt", "a\n");
    let base = commit_all(&repo, "base");
    write(&repo, "a.txt", "tip\n");
    let tip = commit_all(&repo, "tip");
    git(&repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(&repo, "a.txt", "uncommitted\n");

    let error = cli(&repo).rebase_onto(&repo, &tip).unwrap_err();
    assert!(matches!(error, Error::Io(_)), "{error:?}");
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "uncommitted\n"
    );
}

#[test]
fn a_push_after_the_tip_moved_is_rejected_and_an_unreachable_remote_is_an_error() {
    let scratch = Scratch::new("push");
    let (remote, clone) = remote_and_clone(&scratch);
    let git_cli = cli(&clone);
    let start = git_cli.tip("main").unwrap();

    write(&clone, "mine.txt", "mine\n");
    let mine = commit_all(&clone, "mine");
    assert_eq!(
        git_cli.push(&clone, &mine, "main").unwrap(),
        Push::Pushed {
            commit: mine.clone()
        }
    );
    assert_eq!(git_cli.tip("main").unwrap(), mine);

    // Someone else lands first: a push built on the old tip no longer fast-forwards.
    git(
        &clone,
        &["checkout", "--quiet", "-b", "late", start.as_str()],
    );
    write(&clone, "late.txt", "late\n");
    let late = commit_all(&clone, "late");
    let Push::Rejected { reason } = git_cli.push(&clone, &late, "main").unwrap() else {
        panic!("expected a rejected push");
    };
    assert!(reason.contains("rejected"), "{reason}");
    assert_eq!(git_cli.tip("main").unwrap(), mine);

    // Pushing to a new branch creates it.
    assert_eq!(
        git_cli.push(&clone, &late, "other").unwrap(),
        Push::Pushed {
            commit: late.clone()
        }
    );
    assert_eq!(git_cli.tip("other").unwrap(), late);
    assert!(matches!(git_cli.tip("missing"), Err(Error::NotFound(_))));

    fs::remove_dir_all(&remote).unwrap();
    assert!(matches!(
        git_cli.push(&clone, &late, "main"),
        Err(Error::Unreachable(_))
    ));
    assert!(matches!(git_cli.tip("main"), Err(Error::Unreachable(_))));
}

#[test]
fn changed_lists_paths_overall_and_per_landing() {
    let scratch = Scratch::new("changed");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "keep.txt", "k\n");
    write(&repo, "old name.txt", "renamed\n");
    let start = commit_all(&repo, "start");
    // The first landing is two commits; the second is one; one commit has not been recorded
    // as a landing yet.
    write(&repo, "src/a.rs", "a\n");
    commit_all(&repo, "first landing, part one");
    git(&repo, &["mv", "old name.txt", "new name.txt"]);
    let first_end = commit_all(&repo, "first landing, part two");
    write(&repo, "src/a.rs", "a2\n");
    write(&repo, "docs/b.md", "b\n");
    let second_end = commit_all(&repo, "second landing");
    let empty = commit_all(&repo, "unrecorded, changes nothing");

    assert_eq!(
        cli(&repo).changed(&start, &empty).unwrap(),
        ["docs/b.md", "new name.txt", "old name.txt", "src/a.rs"]
    );

    let short = Commit::new(&first_end.as_str()[..10]).unwrap();
    let ends = [second_end.clone(), short, start.clone()];
    let landings = changed::landings(&repo, &start, &empty, &ends).unwrap();
    assert_eq!(landings.len(), 3);
    assert_eq!(landings[0].commits.len(), 2);
    assert_eq!(landings[0].commits[1], first_end);
    assert_eq!(
        landings[0].paths,
        ["new name.txt", "old name.txt", "src/a.rs"]
    );
    assert_eq!(landings[1].commits, [second_end]);
    assert_eq!(landings[1].paths, ["docs/b.md", "src/a.rs"]);
    assert_eq!(landings[2].commits, std::slice::from_ref(&empty));
    assert!(landings[2].paths.is_empty());

    assert!(
        changed::landings(&repo, &empty, &empty, &ends)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_branch_is_read_and_a_remote_branch_deleted_once() {
    let scratch = Scratch::new("branch-delete");
    let (remote, clone) = remote_and_clone(&scratch);
    assert_eq!(refs::branch(&clone).unwrap().as_deref(), Some("main"));
    git(
        &clone,
        &["push", "--quiet", "origin", "HEAD:refs/heads/ignite/a.1"],
    );
    ignite_git::push::delete(&clone, "origin", "ignite/a.1").unwrap();
    assert_eq!(git(&remote, &["branch", "--list", "ignite/a.1"]).trim(), "");
    ignite_git::push::delete(&clone, "origin", "ignite/a.1").unwrap();
    git(&clone, &["checkout", "--quiet", "--detach"]);
    assert_eq!(refs::branch(&clone).unwrap(), None);
}

#[test]
fn uncommitted_lists_edits_and_untracked_files_but_not_ignored_ones() {
    let scratch = Scratch::new("uncommitted");
    let (_, clone) = remote_and_clone(&scratch);
    write(&clone, ".gitignore", "out/\n");
    commit_all(&clone, "ignore out");
    write(&clone, "out/build.log", "x\n");
    write(&clone, "notes.txt", "x\n");
    write(&clone, ".gitignore", "out/\nmore/\n");
    let mut paths = ignite_git::save::uncommitted(&clone).unwrap();
    paths.sort();
    assert_eq!(paths, [".gitignore", "notes.txt"]);
}
