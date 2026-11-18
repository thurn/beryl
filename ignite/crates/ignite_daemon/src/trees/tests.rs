//! Build trees against temporary repositories: a bare remote, and a clone of it as the main
//! checkout, with the fake `Builds` in place of real builds.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ignite_core::fakes::{FakeBuilds, FakeClock};
use ignite_core::ids::{Commit, SessionId, TicketId, TreeId};
use ignite_core::time::Timestamp;
use ignite_core::traits::Error;
use ignite_git::fastforward::{Branch, FastForward};
use ignite_store::local::open_in_memory;

use super::*;

mod owners;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory under the system temp directory, deleted when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "core.hooksPath", "/dev/null"]);
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn commit_all(dir: &Path, message: &str) -> Commit {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "-m", message]);
    Commit::new(git(dir, &["rev-parse", "HEAD"]).trim()).unwrap()
}

/// A remote, a seed checkout that pushes to it, and the main checkout (a clone) with trees.
struct Fixture {
    scratch: Scratch,
    seed: PathBuf,
    repo: PathBuf,
    trees: Trees,
    builds: Arc<FakeBuilds>,
    /// The commits on `main`, oldest first.
    commits: Vec<Commit>,
}

impl Fixture {
    fn new(ticket_trees: u32, commits: usize) -> Fixture {
        Fixture::with_landing(ticket_trees, 1, commits)
    }

    fn with_landing(ticket_trees: u32, landing_trees: u32, commits: usize) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "ignite-trees-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let scratch = Scratch(root.canonicalize().unwrap());
        let remote = scratch.0.join("remote.git");
        let seed = scratch.0.join("seed");
        let repo = scratch.0.join("clone");
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&seed).unwrap();
        git(&remote, &["init", "--quiet", "--bare", "-b", "main"]);
        git(&seed, &["init", "--quiet", "-b", "main"]);
        configure(&seed);
        fs::write(seed.join(".gitignore"), "/out/\n").unwrap();
        let mut made = Vec::new();
        for n in 0..commits {
            write(&seed, "a.txt", &format!("{n}\n"));
            made.push(commit_all(&seed, &format!("commit {n}")));
        }
        git(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&seed, &["push", "--quiet", "origin", "main"]);
        git(
            &scratch.0,
            &["clone", "--quiet", remote.to_str().unwrap(), "clone"],
        );
        configure(&repo);

        let config = TreeConfig {
            repo: repo.clone(),
            dir: repo.join(".worktrees"),
            remote: "origin".to_owned(),
            branches: vec!["main".to_owned()],
            ticket_trees,
            landing_trees,
            disk_limit: None,
            keep: vec!["out".to_owned()],
            prebuild: vec!["bin".to_owned()],
        };
        let clock = Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        ));
        let builds = Arc::new(FakeBuilds::default());
        let trees = Trees::new(config, open_in_memory().unwrap(), clock, builds.clone());
        trees.setup(&made[0]).unwrap();
        Fixture {
            scratch,
            seed,
            repo,
            trees,
            builds,
            commits: made,
        }
    }

    fn tip(&self) -> Commit {
        self.commits.last().unwrap().clone()
    }

    fn request(&self, ticket: &str, compiles: bool) -> LeaseRequest {
        LeaseRequest {
            ticket: TicketId::new(ticket).unwrap(),
            session: Some(SessionId::new(format!("local_{}", ticket.replace('.', "_"))).unwrap()),
            branch: format!("work/{ticket}"),
            at: self.tip(),
            tip: self.tip(),
            compiles,
        }
    }

    /// Leases a tree that must be free right away.
    fn lease(&self, ticket: &str, compiles: bool) -> TreeLease {
        match self.trees.lease(self.request(ticket, compiles)).unwrap() {
            Lease::Ready(lease) => lease,
            Lease::Waiting(waiter) => panic!("expected a free tree: {}", waiter.reason()),
        }
    }

    fn state(&self, tree: &str) -> String {
        let status = self.trees.status().unwrap();
        status
            .trees
            .into_iter()
            .find(|t| t.tree.as_str() == tree)
            .unwrap()
            .state
    }
}

fn id(tree: &str) -> TreeId {
    TreeId::new(tree).unwrap()
}

#[test]
fn a_compiling_ticket_gets_the_tree_nearest_the_tip_and_any_other_the_oldest() {
    let f = Fixture::new(3, 3);
    // tree-01 last built two commits behind the tip, tree-02 one behind, tree-03 never.
    f.trees.record_build(&id("tree-01"), &f.commits[0]).unwrap();
    f.trees.record_build(&id("tree-02"), &f.commits[1]).unwrap();

    let tools = f.lease("T.1", false);
    assert_eq!(tools.tree.as_str(), "tree-03");
    let rust = f.lease("T.2", true);
    assert_eq!(rust.tree.as_str(), "tree-02");
    let last = f.lease("T.3", false);
    assert_eq!(last.tree.as_str(), "tree-01");

    // The lease checked out the ticket's branch at the tip.
    assert_eq!(
        git(&rust.path, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "work/T.2"
    );
    assert_eq!(
        git(&rust.path, &["rev-parse", "HEAD"]).trim(),
        f.tip().as_str()
    );
}

#[test]
fn a_release_saves_the_work_keeps_the_build_directories_and_removes_untracked_files() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    write(&lease.path, "committed.txt", "kept in a commit\n");
    commit_all(&lease.path, "feat: one");
    write(&lease.path, "a.txt", "edited, not committed\n");
    write(&lease.path, "new.txt", "never added\n");
    write(&lease.path, "out/artifact", "built\n");

    let released = f.trees.release(&lease.tree, &f.tip(), None).unwrap();

    // The remote branch holds the commit and, on top of it, the uncommitted work.
    let branch = "refs/heads/ignite/T.1";
    let remote = f.scratch.0.join("remote.git");
    assert_eq!(
        git(&remote, &["rev-parse", branch]).trim(),
        released.saved.commit.as_str()
    );
    assert_eq!(
        git(&remote, &["show", &format!("{branch}:a.txt")]),
        "edited, not committed\n"
    );
    assert_eq!(
        git(&remote, &["show", &format!("{branch}:new.txt")]),
        "never added\n"
    );
    assert_eq!(
        git(&remote, &["show", &format!("{branch}~1:committed.txt")]),
        "kept in a commit\n"
    );
    assert!(git(&remote, &["log", "-1", "--format=%B", branch]).contains(SAVED_TRAILER));
    let mut uncommitted = released.saved.uncommitted.clone();
    uncommitted.sort();
    assert_eq!(uncommitted, ["a.txt", "new.txt"]);

    // The tree is a clean checkout at the tip that kept its build directory.
    assert_eq!(
        fs::read_to_string(lease.path.join("out/artifact")).unwrap(),
        "built\n"
    );
    assert!(!lease.path.join("new.txt").exists());
    assert!(!lease.path.join("committed.txt").exists());
    assert_eq!(git(&lease.path, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&lease.path, &["rev-parse", "HEAD"]).trim(),
        f.tip().as_str()
    );
    assert_eq!(f.state("tree-01"), "free");
}

#[test]
fn a_failed_push_leaves_the_tree_untouched_and_leased() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    write(&lease.path, "a.txt", "work in progress\n");
    write(&lease.path, "new.txt", "untracked\n");
    let status = git(&lease.path, &["status", "--porcelain"]);
    let missing = f.scratch.0.join("no-such-remote.git");
    git(
        &f.repo,
        &["remote", "set-url", "origin", missing.to_str().unwrap()],
    );

    let error = f.trees.release(&lease.tree, &f.tip(), None).unwrap_err();

    assert!(matches!(error, Error::Unreachable(_)), "{error:?}");
    assert_eq!(f.state("tree-01"), "leased");
    assert_eq!(git(&lease.path, &["status", "--porcelain"]), status);
    assert_eq!(
        fs::read_to_string(lease.path.join("a.txt")).unwrap(),
        "work in progress\n"
    );
    assert_eq!(
        git(&lease.path, &["rev-parse", "HEAD"]).trim(),
        f.tip().as_str()
    );
}

#[test]
fn a_lease_with_no_free_tree_waits_and_is_served_by_the_next_release() {
    let f = Fixture::new(1, 1);
    let first = f.lease("T.1", false);
    let Lease::Waiting(waiter) = f.trees.lease(f.request("T.2", false)).unwrap() else {
        panic!("the only ticket tree is leased");
    };
    assert!(
        waiter.reason().contains("tree-01 T.1"),
        "{}",
        waiter.reason()
    );
    assert!(waiter.wait(Duration::ZERO).is_none());
    assert_eq!(
        f.trees.status().unwrap().waiting,
        [TicketId::new("T.2").unwrap()]
    );

    f.trees.release(&first.tree, &f.tip(), None).unwrap();

    let served = waiter.wait(Duration::ZERO).unwrap().unwrap();
    assert_eq!(served.tree.as_str(), "tree-01");
    assert_eq!(
        git(&served.path, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "work/T.2"
    );
    let status = f.trees.status().unwrap();
    assert!(status.waiting.is_empty());
    assert_eq!(status.trees[0].ticket.as_deref(), Some("T.2"));
}

#[test]
fn a_waiter_that_gives_up_leaves_the_queue() {
    let f = Fixture::new(1, 1);
    let first = f.lease("T.1", false);
    let Lease::Waiting(waiter) = f.trees.lease(f.request("T.2", false)).unwrap() else {
        panic!("the only ticket tree is leased");
    };
    drop(waiter);
    f.trees.release(&first.tree, &f.tip(), None).unwrap();
    assert_eq!(f.state("tree-01"), "free");
}

/// macOS only: Windows finds a process inside a tree by its executable alone, which a test
/// would have to copy into the tree; `ignite_os`'s Windows test covers that.
#[cfg(unix)]
#[test]
fn a_release_ends_a_server_left_in_the_tree_but_not_the_caller() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let mut server = Command::new("/bin/sleep")
        .arg("1000")
        .current_dir(&lease.path)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    // The caller stands for the `ignite release` client, which runs from inside the tree.
    let mut caller = Command::new("/bin/sh")
        .args(["-c", "read line"])
        .current_dir(&lease.path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    let released = f
        .trees
        .release(&lease.tree, &f.tip(), Some(caller.id()))
        .unwrap();

    let ended: Vec<u32> = released.ended.iter().map(|e| e.pid).collect();
    assert_eq!(ended, [server.id()]);
    assert!(!server.wait().unwrap().success());
    assert!(caller.try_wait().unwrap().is_none(), "the caller was ended");
    drop(caller.stdin.take());
    caller.wait().unwrap();
}

/// The client usually runs in a pipeline from a shell inside the tree, as in
/// `ignite close -m ... | tail -4`. The other side of the pipe is in the client's process group
/// but is not its ancestor; ending it would end the whole command line.
#[cfg(unix)]
#[test]
fn a_release_spares_the_rest_of_the_callers_command_line() {
    use std::os::unix::process::CommandExt as _;

    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let mut server = Command::new("/bin/sleep")
        .arg("1000")
        .current_dir(&lease.path)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    // The caller leads a process group of its own, as a shell's command line does.
    let mut caller = Command::new("/bin/sh")
        .args(["-c", "read line"])
        .current_dir(&lease.path)
        .stdin(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let group = i32::try_from(caller.id()).unwrap();
    let mut tail = Command::new("/bin/sh")
        .args(["-c", "read line"])
        .current_dir(&lease.path)
        .stdin(Stdio::piped())
        .process_group(group)
        .spawn()
        .unwrap();

    let released = f
        .trees
        .release(&lease.tree, &f.tip(), Some(caller.id()))
        .unwrap();

    let ended: Vec<u32> = released.ended.iter().map(|e| e.pid).collect();
    assert_eq!(ended, [server.id()]);
    assert!(!server.wait().unwrap().success());
    assert!(
        tail.try_wait().unwrap().is_none(),
        "the pipe's other side was ended"
    );
    for child in [&mut caller, &mut tail] {
        drop(child.stdin.take());
        child.wait().unwrap();
    }
}

#[test]
fn no_directory_outside_the_configured_trees_is_created() {
    let f = Fixture::new(2, 2);
    let before: Vec<_> = listing(&f.scratch.0);
    let a = f.lease("T.1", true);
    let b = f.lease("T.2", false);
    f.trees.release(&a.tree, &f.tip(), None).unwrap();
    f.trees.release(&b.tree, &f.tip(), None).unwrap();
    f.trees.keep_one_built(&f.tip()).unwrap();
    let landing = f
        .trees
        .lease_landing("landing of T.1", &f.tip())
        .unwrap()
        .unwrap();
    f.trees
        .release_landing(&landing.tree, Some(&f.tip()))
        .unwrap();

    assert_eq!(
        listing(&f.repo.join(".worktrees")),
        ["landing-1", "tree-01", "tree-02"]
    );
    assert_eq!(listing(&f.scratch.0), before);
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn only_the_session_that_holds_a_tree_passes_the_fence() {
    let f = Fixture::new(1, 1);
    let lease = f.lease("T.1", false);
    let holder = SessionId::new("local_T_1").unwrap();
    let other = SessionId::new("local_other").unwrap();
    let inside = lease.path.join("sub");
    fs::create_dir_all(&inside).unwrap();

    assert_eq!(
        f.trees.fence(&inside, Some(&holder)).unwrap(),
        Some(lease.tree.clone())
    );
    assert!(matches!(
        f.trees.fence(&inside, Some(&other)),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        f.trees.fence(&inside, None),
        Err(Error::Conflict(_))
    ));
    assert_eq!(f.trees.fence(&f.repo, Some(&other)).unwrap(), None);
    assert_eq!(f.trees.holder(&lease.tree).unwrap(), Some(holder));
}

#[test]
fn one_free_tree_is_kept_built_at_the_tip() {
    let f = Fixture::new(2, 2);
    let built = f.trees.keep_one_built(&f.tip()).unwrap();
    assert_eq!(built, Some(id("tree-01")));
    assert_eq!(f.builds.requests().len(), 1);
    assert_eq!(f.builds.requests()[0].1, "bin");
    assert_eq!(f.state("tree-01"), "free");
    // Already built at the tip: nothing more to do.
    assert_eq!(f.trees.keep_one_built(&f.tip()).unwrap(), None);
    // And the next compiling ticket gets that tree.
    assert_eq!(f.lease("T.1", true).tree.as_str(), "tree-01");
}

#[test]
fn the_main_checkout_is_left_alone_while_it_has_a_local_change() {
    let f = Fixture::new(1, 1);
    write(&f.seed, "a.txt", "newer\n");
    let newer = commit_all(&f.seed, "newer");
    git(&f.seed, &["push", "--quiet", "origin", "main"]);
    write(&f.repo, "a.txt", "a local change\n");

    assert_eq!(f.trees.fast_forward_main().unwrap(), FastForward::Busy);
    assert_eq!(
        git(&f.repo, &["rev-parse", "main"]).trim(),
        f.tip().as_str()
    );

    git(&f.repo, &["checkout", "--quiet", "--", "a.txt"]);
    let FastForward::Branches(moved) = f.trees.fast_forward_main().unwrap() else {
        panic!("the checkout is clean now");
    };
    assert_eq!(
        moved,
        [(
            "main".to_owned(),
            Branch::Moved {
                from: f.tip(),
                to: newer.clone()
            }
        )]
    );
    assert_eq!(fs::read_to_string(f.repo.join("a.txt")).unwrap(), "newer\n");
}

#[test]
fn replaced_builds_are_swept_and_then_the_free_trees_used_longest_ago_are_cleared() {
    let f = Fixture::new(2, 1);
    let profile = f.repo.join(".worktrees/tree-01/out/debug");
    for (unit, text) in [
        ("foo-0a0a", "old"),
        ("foo-0b0b", "new"),
        ("bar-0c0c", "only"),
    ] {
        write(
            &profile,
            &format!(".fingerprint/{unit}/lib-{}", &unit[..3]),
            "",
        );
        write(
            &profile,
            &format!(".fingerprint/{unit}/lib-{}.json", &unit[..3]),
            "{}",
        );
        write(&profile, &format!("deps/lib{unit}.rlib"), text);
        write(&profile, &format!("deps/{unit}.d"), text);
    }
    // The old build of foo was written first.
    let old = fs::File::options()
        .write(true)
        .open(profile.join(".fingerprint/foo-0a0a/lib-foo"))
        .unwrap();
    old.set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
    let old = fs::File::options()
        .write(true)
        .open(profile.join(".fingerprint/foo-0a0a/lib-foo.json"))
        .unwrap();
    old.set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();

    let swept = disk::sweep_superseded(&f.repo.join(".worktrees/tree-01/out"));
    assert_eq!(swept, [profile.join(".fingerprint/foo-0a0a")]);
    assert!(!profile.join("deps/libfoo-0a0a.rlib").exists());
    assert!(!profile.join("deps/foo-0a0a.d").exists());
    assert!(profile.join("deps/libfoo-0b0b.rlib").exists());
    assert!(profile.join("deps/libbar-0c0c.rlib").exists());

    write(
        &f.repo.join(".worktrees/tree-02"),
        "out/big",
        &"x".repeat(100_000),
    );
    let mut config = f.trees.config().clone();
    config.disk_limit = Some(1);
    let small = Trees::new(
        config,
        open_in_memory().unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        f.builds.clone(),
    );
    small.setup(&f.tip()).unwrap();
    let report = small.hold_disk().unwrap();
    assert!(report.before > report.after);
    assert_eq!(report.cleared, [id("tree-01"), id("tree-02")]);
    assert!(!f.repo.join(".worktrees/tree-02/out").exists());
}

#[test]
fn trees_over_the_limit_are_brought_under_it_and_a_leased_tree_is_left_alone() {
    let f = Fixture::new(3, 1);
    for tree in ["tree-01", "tree-02", "tree-03"] {
        write(
            &f.repo.join(".worktrees").join(tree),
            "out/big",
            &"x".repeat(200_000),
        );
    }
    let one_tree = f
        .trees
        .disk_usage()
        .into_iter()
        .map(|(_, b)| b)
        .max()
        .unwrap();

    let mut config = f.trees.config().clone();
    // Room for two and a half trees' builds, with three holding them.
    config.disk_limit = Some(one_tree * 2 + one_tree / 2);
    let limited = Trees::new(
        config,
        open_in_memory().unwrap(),
        Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        )),
        f.builds.clone(),
    );
    limited.setup(&f.tip()).unwrap();
    let leased = match limited.lease(f.request("T.1", false)).unwrap() {
        Lease::Ready(lease) => lease,
        Lease::Waiting(waiter) => panic!("{}", waiter.reason()),
    };

    let report = limited.hold_disk().unwrap();

    assert!(report.before > report.limit.unwrap());
    assert!(report.after <= report.limit.unwrap(), "{report:?}");
    assert!(!report.cleared.is_empty());
    assert!(!report.cleared.contains(&leased.tree));
    assert!(leased.path.join("out/big").exists());
    let states: Vec<String> = limited
        .status()
        .unwrap()
        .trees
        .into_iter()
        .map(|t| t.state)
        .collect();
    assert!(!states.iter().any(|s| s == "resetting"), "{states:?}");
}

#[test]
fn setup_frees_a_landing_tree_left_landing_by_a_stopped_daemon() {
    let f = Fixture::new(1, 1);
    let leased = f
        .trees
        .lease_landing("machine:T.1", &f.tip())
        .unwrap()
        .unwrap();
    assert_eq!(f.state(leased.tree.as_str()), "landing");
    assert!(
        f.trees
            .lease_landing("machine:T.2", &f.tip())
            .unwrap()
            .is_none()
    );

    // The daemon restarts: the holder is gone, so `setup` must free the tree.
    f.trees.setup(&f.tip()).unwrap();

    assert_eq!(f.state(leased.tree.as_str()), "free");
    assert!(
        f.trees
            .lease_landing("machine:T.2", &f.tip())
            .unwrap()
            .is_some()
    );
}

#[test]
fn idle_work_never_takes_the_last_free_landing_tree() {
    let f = Fixture::with_landing(1, 2, 1);
    let idle = f
        .trees
        .lease_spare_landing("idle work", &f.tip())
        .unwrap()
        .expect("two landing trees are free, so idle work may take one");
    assert!(
        f.trees
            .lease_spare_landing("idle work", &f.tip())
            .unwrap()
            .is_none(),
        "idle work took the last free landing tree"
    );
    // Tip verification still finds the tree idle work left.
    let tip = f
        .trees
        .lease_landing("tip verification", &f.tip())
        .unwrap()
        .expect("the last landing tree is left for tip verification");
    assert_ne!(tip.tree, idle.tree);
}

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
