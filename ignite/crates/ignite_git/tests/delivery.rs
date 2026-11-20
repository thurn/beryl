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
fn a_rebase_keeps_the_times_of_files_it_writes_back_unchanged() {
    let scratch = Scratch::new("rebase-times");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "mine.txt", "base\n");
    write(&repo, "theirs.txt", "base\n");
    let base = commit_all(&repo, "base");
    write(&repo, "theirs.txt", "tip\n");
    let tip = commit_all(&repo, "landed");
    git(&repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(&repo, "mine.txt", "work\n");
    commit_all(&repo, "work");
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    for file in ["mine.txt", "theirs.txt"] {
        fs::File::options()
            .write(true)
            .open(repo.join(file))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    let outcome = cli(&repo).rebase_onto(&repo, &tip).unwrap();
    assert!(matches!(outcome, Rebase::Clean { .. }), "{outcome:?}");
    let modified = |file: &str| fs::metadata(repo.join(file)).unwrap().modified().unwrap();
    // The branch's own edit has the content it had, so Cargo must see the time it built.
    assert_eq!(modified("mine.txt"), old);
    // The landed edit is new to this tree, so whatever reads it is rebuilt.
    assert_ne!(modified("theirs.txt"), old);
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

/// Every ref of `repo` and what it points at, to show that nothing moved.
fn refs_of(repo: &std::path::Path) -> String {
    git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"])
}

fn commit_as(dir: &std::path::Path, author: &str, date: &str, message: &str) -> Commit {
    git(dir, &["add", "--all"]);
    git(
        dir,
        &[
            "commit",
            "--quiet",
            &format!("--author={author}"),
            &format!("--date={date}"),
            "-m",
            message,
        ],
    );
    head(dir)
}

/// `base`, then `tip` with two landed commits on `main`, and `work` with three commits on
/// `base`, checked out.
fn branch_and_moved_tip(repo: &std::path::Path) -> (Commit, Commit, Commit) {
    fs::create_dir_all(repo).unwrap();
    init(repo);
    write(repo, "shared.txt", "one\ntwo\nthree\nfour\nfive\n");
    let base = commit_all(repo, "base");
    write(repo, "landed.txt", "landed\n");
    commit_all(repo, "landed one");
    write(
        repo,
        "shared.txt",
        "one\ntwo\nthree\nfour\nfive changed on main\n",
    );
    let tip = commit_all(repo, "landed two");
    git(repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(repo, "a.txt", "a\n");
    commit_as(
        repo,
        "Ann <ann@example.com>",
        "2001-02-03T04:05:06+0100",
        "add a\n\nWith a body.",
    );
    write(
        repo,
        "shared.txt",
        "one changed on work\ntwo\nthree\nfour\nfive\n",
    );
    commit_as(
        repo,
        "Bob <bob@example.com>",
        "2002-03-04T05:06:07-0500",
        "edit shared",
    );
    write(repo, "b.txt", "b\n");
    let work = commit_all(repo, "add b");
    (base, tip, work)
}

#[test]
fn a_replay_matches_git_rebase_and_keeps_authors_and_messages() {
    let scratch = Scratch::new("replay-clean");
    let repo = scratch.path("repo");
    let (_, tip, work) = branch_and_moved_tip(&repo);
    let refs = refs_of(&repo);

    let Rebase::Clean {
        onto,
        head: replayed,
        landed_since,
    } = cli(&repo).replay(&repo, &work, &tip).unwrap()
    else {
        panic!("expected a clean replay");
    };
    assert_eq!((onto, landed_since), (tip.clone(), 2));
    assert_eq!(refs_of(&repo), refs, "a replay moves no ref");
    let replayed_commits = refs::commits_between(&repo, &tip, &replayed).unwrap();
    assert_eq!(replayed_commits.len(), 3);
    let log = |range: &str| {
        git(
            &repo,
            &[
                "log",
                "--reverse",
                "--format=%an <%ae> %ad%n%B",
                "--date=raw",
                range,
            ],
        )
    };
    assert_eq!(
        log(&format!("{tip}..{replayed}")),
        log(&format!("{tip}..{work}"))
    );

    git(&repo, &["rebase", "--quiet", tip.as_str()]);
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{replayed}^{{tree}}")]),
        git(&repo, &["rev-parse", "HEAD^{tree}"]),
        "the replayed tree is the tree git rebase makes"
    );
}

#[test]
fn a_conflicting_replay_names_the_files_and_creates_nothing_a_ref_holds() {
    let scratch = Scratch::new("replay-conflict");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "shared.txt", "base\n");
    write(&repo, "other.txt", "base\n");
    let base = commit_all(&repo, "base");
    write(&repo, "shared.txt", "tip\n");
    let tip = commit_all(&repo, "tip");
    git(&repo, &["checkout", "--quiet", "-b", "work", base.as_str()]);
    write(&repo, "other.txt", "work\n");
    commit_all(&repo, "clean");
    write(&repo, "shared.txt", "work\n");
    let work = commit_all(&repo, "conflicting");
    let refs = refs_of(&repo);

    assert_eq!(
        cli(&repo).replay(&repo, &work, &tip).unwrap(),
        Rebase::Conflict {
            files: vec!["shared.txt".to_owned()]
        }
    );
    assert_eq!(refs_of(&repo), refs);
    assert_eq!(head(&repo), work);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn a_branch_whose_change_already_landed_replays_to_the_tip() {
    let scratch = Scratch::new("replay-landed");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "a.txt", "a\n");
    let base = commit_all(&repo, "base");
    git(&repo, &["checkout", "--quiet", "-b", "work"]);
    write(&repo, "fix.txt", "fixed\n");
    let work = commit_all(&repo, "fix");
    git(&repo, &["checkout", "--quiet", "main"]);
    write(&repo, "earlier.txt", "earlier\n");
    commit_all(&repo, "earlier");
    git(&repo, &["cherry-pick", work.as_str()]);
    write(&repo, "later.txt", "later\n");
    let tip = commit_all(&repo, "later");
    assert_ne!(base, tip);

    assert_eq!(
        cli(&repo).replay(&repo, &work, &tip).unwrap(),
        Rebase::Clean {
            onto: tip.clone(),
            head: tip,
            landed_since: 3
        }
    );
}

#[test]
fn a_replay_leaves_the_working_tree_index_and_head_byte_for_byte_unchanged() {
    let scratch = Scratch::new("replay-untouched");
    let repo = scratch.path("repo");
    let (_, tip, work) = branch_and_moved_tip(&repo);
    write(&repo, "a.txt", "uncommitted\n");
    write(&repo, "untracked.txt", "untracked\n");
    let files = [
        "a.txt",
        "b.txt",
        "shared.txt",
        "untracked.txt",
        ".git/index",
        ".git/HEAD",
    ];
    let read = || -> Vec<(Vec<u8>, std::time::SystemTime)> {
        files
            .iter()
            .map(|file| {
                let path = repo.join(file);
                (
                    fs::read(&path).unwrap(),
                    fs::metadata(&path).unwrap().modified().unwrap(),
                )
            })
            .collect()
    };
    // `git status` refreshes the index file, so it runs before the snapshot of the files.
    let status = git(&repo, &["status", "--porcelain"]);
    let before = read();

    let outcome = cli(&repo).replay(&repo, &work, &tip).unwrap();
    assert!(matches!(outcome, Rebase::Clean { .. }), "{outcome:?}");
    assert_eq!(read(), before);
    assert_eq!(head(&repo), work);
    assert_eq!(git(&repo, &["status", "--porcelain"]), status);
    assert!(!repo.join("landed.txt").exists());
}

#[test]
fn a_revert_of_two_commits_on_a_later_tip_removes_exactly_their_change() {
    let scratch = Scratch::new("revert-clean");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "kept.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
    commit_all(&repo, "base");
    write(&repo, "added.txt", "added\n");
    let first = commit_all(&repo, "landing one");
    write(
        &repo,
        "kept.txt",
        "one changed\ntwo\nthree\nfour\nfive\nsix\n",
    );
    let second = commit_all(&repo, "landing two");
    write(
        &repo,
        "kept.txt",
        "one changed\ntwo\nthree\nfour\nfive\nsix changed later\n",
    );
    write(&repo, "later.txt", "later\n");
    let tip = commit_all(&repo, "later landing");
    let refs = refs_of(&repo);

    let ignite_core::traits::RevertOutcome::Clean { commit } = cli(&repo)
        .revert(
            &repo,
            &tip,
            &[first, second],
            "revert: landing one\n\nThe build broke.",
        )
        .unwrap()
    else {
        panic!("expected a clean revert");
    };
    assert_eq!(refs_of(&repo), refs);
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{commit}^")]).trim(),
        tip.as_str()
    );
    assert_eq!(
        git(
            &repo,
            &["show", "-s", "--format=%an <%ae>%n%B", commit.as_str()]
        ),
        "ignite <ignite@localhost>\nrevert: landing one\n\nThe build broke.\n\n"
    );
    assert_eq!(
        changed::between(&repo, &tip, &commit).unwrap(),
        vec!["added.txt".to_owned(), "kept.txt".to_owned()]
    );
    assert_eq!(
        git(&repo, &["show", &format!("{commit}:kept.txt")]),
        "one\ntwo\nthree\nfour\nfive\nsix changed later\n"
    );
}

#[test]
fn a_revert_that_conflicts_with_a_later_commit_names_the_files() {
    let scratch = Scratch::new("revert-conflict");
    let repo = scratch.path("repo");
    fs::create_dir_all(&repo).unwrap();
    init(&repo);
    write(&repo, "x.txt", "base\n");
    commit_all(&repo, "base");
    write(&repo, "x.txt", "landing\n");
    write(&repo, "y.txt", "landing\n");
    let landing = commit_all(&repo, "landing");
    write(&repo, "x.txt", "later\n");
    let tip = commit_all(&repo, "later");

    assert_eq!(
        cli(&repo)
            .revert(&repo, &tip, &[landing], "revert: landing")
            .unwrap(),
        ignite_core::traits::RevertOutcome::Conflict {
            files: vec!["x.txt".to_owned()]
        }
    );
}

#[test]
fn merge_base_committed_at_and_tree_id_read_the_repository() {
    let scratch = Scratch::new("objects");
    let repo = scratch.path("repo");
    let (base, tip, work) = branch_and_moved_tip(&repo);
    let git_cli = cli(&repo);
    assert_eq!(git_cli.merge_base(&repo, &work, &tip).unwrap(), base);
    let seconds: i64 = git(&repo, &["show", "-s", "--format=%ct", tip.as_str()])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        git_cli.committed_at(&repo, &tip).unwrap().millis(),
        seconds * 1000
    );

    write(&repo, "new.txt", "new\n");
    let id = git_cli.tree_id(&repo).unwrap();
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? new.txt\n");
    git(&repo, &["add", "--all"]);
    assert_eq!(git(&repo, &["write-tree"]).trim(), id);
}
