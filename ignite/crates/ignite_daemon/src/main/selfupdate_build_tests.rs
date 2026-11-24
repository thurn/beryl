use std::fs;
use std::process::Command;

use ignite_core::protocol::Usage;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};

use super::*;
use crate::update::{ReleaseBuilder, TreeBuilder};

/// A daemon, as [`daemon`] makes it, with the fake hub and selftest it was given.
fn daemon_with_selftest() -> (
    SelfUpdate,
    Arc<FakeGit>,
    FakeRestart,
    Arc<FakeHub>,
    Arc<FakeSelftest>,
) {
    let (mut update, git, restart) = daemon(NEW);
    let hub = Arc::new(FakeHub::default());
    let selftest = Arc::new(FakeSelftest::default());
    update.hub = hub.clone();
    update.selftest = selftest.clone();
    git.set_changed(&commit(OLD), &commit(NEW), &["ignite/crates/x.rs"]);
    (update, git, restart, hub, selftest)
}

#[test]
fn a_release_that_passes_its_selftest_restarts_and_is_checked_once() {
    let (update, _git, restart, hub, selftest) = daemon_with_selftest();

    // The hub's version is asked for on every heartbeat, and the tip loop asks too; one selftest
    // runs however often.
    update.after_heartbeat(Some(NEW), IDLE);
    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(NEW), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(NEW))]);
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
    assert!(hub.justiciar_reports().is_empty());
    assert_eq!(update.store.current_commit(), Some(commit(NEW)));
}

#[test]
fn a_release_that_fails_its_selftest_is_never_restarted_into_and_is_reported_once() {
    let (update, git, restart, hub, selftest) = daemon_with_selftest();
    selftest.fails.lock().unwrap().push(NEW.to_owned());

    // From the tip loop, the poll and the hub's version alike.
    update.after_master_moved().unwrap();
    settle(&update);
    for _ in 0..3 {
        poll(&update);
        update.after_heartbeat(Some(NEW), IDLE);
        update.after_heartbeat(Some(OLD), IDLE);
        settle(&update);
    }

    assert!(restart.started().is_empty());
    assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
    assert!(update.store.is_rejected(&commit(NEW)));
    // `bin/current` was never pointed at it.
    assert_eq!(update.store.current_commit(), None);
    let reports = hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(reports[0].details.contains("cannot parse the manifest"));
    assert_eq!(reports[0].subject, format!("release-selftest:{NEW}"));

    // A newer release that passes is restarted into.
    let newer = "f00dfee";
    git.set_tip("master", commit(newer));
    git.set_changed(&commit(OLD), &commit(newer), &["ignite/crates/x.rs"]);
    poll(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(restart.started(), [update.store.path(&commit(newer))]);
    assert_eq!(hub.justiciar_reports().len(), 1);
}

#[test]
fn a_selftest_report_the_hub_did_not_take_is_sent_when_it_answers() {
    let (update, _git, restart, hub, selftest) = daemon_with_selftest();
    selftest.fails.lock().unwrap().push(NEW.to_owned());
    hub.set_unreachable(true);

    update.after_master_moved().unwrap();
    settle(&update);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(hub.justiciar_reports().is_empty());
    assert!(update.store.is_rejected(&commit(NEW)));

    hub.set_unreachable(false);
    update.after_heartbeat(Some(OLD), IDLE);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(hub.justiciar_reports().len(), 1);
    assert!(restart.started().is_empty());
}

const LINK_ERROR: &str = "rust-lld: error: undefined symbol: example::release";
const RETRY_ERROR: &str = "rust-lld: error: undefined symbol: example::retry";

/// A scripted Cargo attempt that checks the target and lease before writing the log.
struct CargoAttempt {
    work: PathBuf,
    target: &'static str,
    log: &'static str,
    error: Option<&'static str>,
    calls: Mutex<Vec<CommandSpec>>,
}

impl Launcher for CargoAttempt {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.calls.lock().unwrap().push(spec.clone());
        let target = spec.cwd.join(self.target);
        assert_eq!(spec.args.last().map(Path::new), Some(target.as_path()));
        let marker = fs::read_to_string(self.work.join("release-build")).unwrap();
        assert_eq!(spec.env[0], ("IGNITE_RELEASE_BUILD".to_owned(), marker));
        let lock = fs::OpenOptions::new()
            .write(true)
            .open(self.work.join("release-build.lock"))
            .unwrap();
        assert!(matches!(lock.try_lock(), Err(fs::TryLockError::WouldBlock)));
        if self.target == "target-retry" {
            assert_eq!(
                spec.env[1..],
                [(
                    "CARGO_BUILD_BUILD_DIR".to_owned(),
                    target.display().to_string()
                )]
            );
            assert!(fs::read_dir(&target).unwrap().next().is_none());
            assert!(spec.cwd.join("target/cached-artifact").is_file());
        } else {
            assert_eq!(spec.env.len(), 1);
            assert!(target.join("cached-artifact").is_file());
        }

        let output = self.work.join(self.log);
        let exit = if let Some(error) = self.error {
            fs::write(&output, error).unwrap();
            101
        } else {
            let release = target.join("release");
            fs::create_dir_all(&release).unwrap();
            let exe = release.join(format!("ignite{}", std::env::consts::EXE_SUFFIX));
            fs::write(exe, "fresh executable").unwrap();
            fs::write(&output, "build succeeded").unwrap();
            0
        };
        RunResult {
            exit: Some(exit),
            output,
            usage: Usage::default(),
            hung: false,
        }
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn release_repo() -> (PathBuf, Commit) {
    let repo = tempdir();
    git(&repo, &["init", "--quiet", "-b", "test"]);
    fs::create_dir(repo.join("ignite")).unwrap();
    fs::write(repo.join("ignite/Cargo.toml"), "").unwrap();
    git(&repo, &["add", "--force", "ignite/Cargo.toml"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Test release",
        ],
    );
    let release = commit(&git(&repo, &["rev-parse", "HEAD"]));
    (repo, release)
}

#[test]
fn a_windows_link_failure_retries_once_and_reaches_selftest_or_keeps_both_errors() {
    for retry_error in [None, Some(RETRY_ERROR)] {
        let (repo, release) = release_repo();
        let (mut update, _, _, _, selftest) = daemon_with_selftest();
        let work = tempdir();
        let tree = work.join(TreeBuilder::TREE);
        ignite_git::worktree::add(&repo, &tree, "ignite-update", &release).unwrap();
        let workspace = tree.join("ignite");
        fs::create_dir(workspace.join("target")).unwrap();
        fs::write(workspace.join("target/cached-artifact"), "cached").unwrap();
        fs::create_dir(workspace.join("target-retry")).unwrap();
        fs::write(workspace.join("target-retry/old-artifact"), "old").unwrap();

        let cached = Arc::new(CargoAttempt {
            work: work.clone(),
            target: "target",
            log: "build.log",
            error: Some(LINK_ERROR),
            calls: Mutex::default(),
        });
        let retry = Arc::new(CargoAttempt {
            work: work.clone(),
            target: "target-retry",
            log: "build.retry.log",
            error: retry_error,
            calls: Mutex::default(),
        });
        update.builder = Arc::new(ReleaseBuilder {
            tree: TreeBuilder {
                repo,
                work: work.clone(),
            },
            launcher: cached.clone(),
            retry_launcher: Some(retry.clone()),
        });

        update.start_build(&release);
        let task = update.building().take().unwrap();
        let result = task.handle.join().unwrap();

        let cached_calls = cached.calls.lock().unwrap();
        let retry_calls = retry.calls.lock().unwrap();
        let ([first], [second]) = (&cached_calls[..], &retry_calls[..]) else {
            panic!("Expected one cached build and one retry");
        };
        assert_eq!(first.cwd, second.cwd);
        assert_eq!(first.env[0], second.env[0]);
        assert_eq!(first.env[0].1, release.as_str());
        assert!(!work.join("release-build").exists());
        assert_eq!(
            fs::read_to_string(work.join(cached.log)).unwrap(),
            LINK_ERROR
        );

        if let Some(retry_error) = retry_error {
            let Err(Failure::Build(error)) = result else {
                panic!("Expected the retry to return a build failure");
            };
            let error = error.to_string();
            for diagnostic in [LINK_ERROR, RETRY_ERROR, cached.log, retry.log] {
                assert!(error.contains(diagnostic), "{error}");
            }
            assert_eq!(
                fs::read_to_string(work.join(retry.log)).unwrap(),
                retry_error
            );
            assert_eq!(selftest.runs.load(Ordering::SeqCst), 0);
            assert!(!update.store.has(&release));
        } else {
            let stored = update.store.path(&release);
            assert!(matches!(result, Ok(path) if path == stored));
            assert_eq!(selftest.runs.load(Ordering::SeqCst), 1);
            assert_eq!(fs::read_to_string(stored).unwrap(), "fresh executable");
        }
    }
}
