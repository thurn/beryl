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

mod elastic;
mod landing;
mod owners;
mod releases;
mod retire;
mod seed;
mod setup;
mod slots;

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
    clock: Arc<FakeClock>,
    /// The commits on `main`, oldest first.
    commits: Vec<Commit>,
}

impl Fixture {
    fn new(ticket_trees: u32, commits: usize) -> Fixture {
        Fixture::with_landing(ticket_trees, 1, commits)
    }

    fn with_landing(ticket_trees: u32, landing_trees: u32, commits: usize) -> Fixture {
        let root = ignite_core::test_files::temp_dir().join(format!(
            "ignite-trees-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // `/var` is a link on macOS, and git reports real paths.
        let scratch = Scratch(ignite_os::paths::canonicalize(&root).unwrap());
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
            saved: "ignite/".to_owned(),
            branches: vec!["main".to_owned()],
            ticket_tree_ceiling: ticket_trees,
            landing_trees,
            keep: vec!["out".to_owned()].into(),
            prebuild: vec!["bin".to_owned()].into(),
            compiled_outputs: BTreeSet::from(["out".to_owned()]).into(),
            prebuild_outputs: BTreeMap::from([("bin".to_owned(), vec!["out".to_owned()])]).into(),
        };
        let clock = Arc::new(FakeClock::new(
            Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
        ));
        let builds = Arc::new(FakeBuilds::default());
        let trees = Trees::new(
            config,
            open_in_memory().unwrap(),
            clock.clone(),
            builds.clone(),
        );
        trees.setup(&made[0]).unwrap();
        for _ in 0..ticket_trees {
            let locked = trees.lock();
            let rows = store::all(&locked.db).unwrap();
            trees
                .create_ticket_tree(&locked, &trees.config().dir, &rows, &made[0])
                .unwrap()
                .unwrap();
        }
        Fixture {
            scratch,
            seed,
            repo,
            trees,
            builds,
            clock,
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
            files: Vec::new(),
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
fn a_compiling_ticket_prefers_a_tree_holding_the_build_directory_beside_its_files() {
    let f = Fixture::new(2, 2);
    // tree-01 last built at the tip, tree-02 one commit behind; only tree-02 holds `out`.
    f.trees.record_build(&id("tree-01"), &f.commits[1]).unwrap();
    f.trees.record_build(&id("tree-02"), &f.commits[0]).unwrap();
    write(&f.repo.join(".worktrees/tree-02"), "out/built", "x");

    let mut request = f.request("T.1", true);
    request.files = vec!["src/lib.rs".to_owned()];
    let Lease::Ready(lease) = f.trees.lease(request).unwrap() else {
        panic!("a tree is free");
    };
    assert_eq!(lease.tree.as_str(), "tree-02");
    // A compiling ticket that names no file still gets the tree nearest the tip.
    assert_eq!(f.lease("T.2", true).tree.as_str(), "tree-01");
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
fn a_saved_branch_with_no_commits_past_the_tip_starts_the_tree_at_the_tip() {
    let f = Fixture::new(1, 3);
    let mut request = f.request("T.9", false);
    // An earlier attempt saved the branch at an older commit and added nothing to it.
    request.at = f.commits[0].clone();
    let Lease::Ready(lease) = f.trees.lease(request).unwrap() else {
        panic!("expected a free tree");
    };
    assert_eq!(lease.at, f.tip());
    assert_eq!(
        git(&lease.path, &["rev-parse", "HEAD"]).trim(),
        f.tip().as_str()
    );
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
    if let Some(prebuild) = f.trees.start_idle_prebuild(&f.tip()).unwrap() {
        prebuild.join().unwrap().unwrap();
    }
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
    let prebuild = f.trees.start_idle_prebuild(&f.tip()).unwrap().unwrap();
    prebuild.join().unwrap().unwrap();
    assert_eq!(f.builds.requests().len(), 1);
    assert_eq!(f.builds.requests()[0].1, "bin");
    assert_eq!(f.state("tree-01"), "free");
    // Already built at the tip: nothing more to do.
    assert!(f.trees.start_idle_prebuild(&f.tip()).unwrap().is_none());
    // And the next compiling ticket gets that tree.
    assert_eq!(f.lease("T.1", true).tree.as_str(), "tree-01");
}

#[test]
fn an_idle_prebuild_can_wait_for_disk_space_after_its_caller_returns() {
    use std::sync::mpsc::{Receiver, Sender, channel};

    use ignite_core::protocol::Class;

    struct WaitingBuild {
        waiting: Sender<Class>,
        disk_room: Mutex<Receiver<()>>,
    }

    impl Builds for WaitingBuild {
        fn ensure(&self, _: &TreeId, _: &str, _: &[String]) -> BuildResult {
            let _wait = crate::steps::waiting("disk space");
            self.waiting.send(crate::steps::class().unwrap()).unwrap();
            self.disk_room.lock().unwrap().recv().unwrap();
            BuildResult::Current
        }
    }

    let mut f = Fixture::new(1, 2);
    let (waiting, started) = channel();
    let (give_room, disk_room) = channel();
    Arc::get_mut(&mut f.trees.inner).unwrap().builds = Arc::new(WaitingBuild {
        waiting,
        disk_room: Mutex::new(disk_room),
    });

    let prebuild = f.trees.start_idle_prebuild(&f.tip()).unwrap().unwrap();
    assert_eq!(started.recv().unwrap(), Class::Idle);
    assert_eq!(f.state("tree-01"), "prebuilding");
    assert!(!prebuild.is_finished());

    give_room.send(()).unwrap();
    prebuild.join().unwrap().unwrap();
    assert_eq!(f.state("tree-01"), "free");
    assert!(f.trees.start_idle_prebuild(&f.tip()).unwrap().is_none());
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
fn a_branch_the_remote_lacks_is_skipped_and_the_others_still_move() {
    // `repository.green` is fast-forwarded with the integration branches, but the remote has
    // no `green` until a tip round's build checks first pass. Its fetch failing with "couldn't
    // find remote ref" once failed every round of the daemon's main loop.
    let f = Fixture::new(1, 1);
    write(&f.seed, "a.txt", "newer\n");
    let newer = commit_all(&f.seed, "newer");
    git(&f.seed, &["push", "--quiet", "origin", "main"]);

    let branches = ["main".to_owned(), "green".to_owned()];
    let FastForward::Branches(moved) =
        ignite_git::fastforward::main_checkout(&f.repo, "origin", &branches).unwrap()
    else {
        panic!("the checkout is clean");
    };
    assert_eq!(
        moved,
        [
            (
                "main".to_owned(),
                Branch::Moved {
                    from: f.tip(),
                    to: newer
                }
            ),
            ("green".to_owned(), Branch::NotOnRemote),
        ]
    );
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
