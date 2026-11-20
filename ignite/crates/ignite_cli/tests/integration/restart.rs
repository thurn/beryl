//! A daemon restarted under load loses no work: a real `ignite daemon` process, an `ignite test`
//! and an `ignite land` whose check commands wait on a file the test controls, and
//! `ignite internal restart` while they wait. The clients print their ordinary replies, every
//! check command ran once, each check has one result, and the landing pushed once.
//!
//! The world is a temporary Ignite home, a bare repository standing in for the remote, a hub
//! served by the test's process on a random port, and one check family, `slow`, whose shell
//! adapter logs each run and then waits until the file `gate` exists. Every wait in the test is
//! for a condition (a file, a line, a process), never for a length of time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use ignite_core::config::Config;
use ignite_daemon::doctor::{Host as _, NativeHost};
use ignite_hub::server::{HubServer, RunningHub};

/// How long any one wait may last before the test fails as hung.
const HUNG: Duration = Duration::from_mins(5);

/// The family's adapter. `run` logs `<class> <check> <checksum of src>` for each check to
/// `@MARKS@/runs.log`, waits until `@GATE@` exists, then passes them.
const ADAPTER: &str = r#"#!/bin/sh
case "$1" in
list)
  echo '{"check": "slow:one"}'
  echo '{"check": "slow:two"}' ;;
run)
  shift
  sum=$(cat src/* | cksum | cut -d ' ' -f 1)
  for check in "$@"; do echo "$IGNITE_CLASS $check $sum" >> '@MARKS@/runs.log'; done
  while [ ! -e '@GATE@' ]; do sleep 0.02; done
  for check in "$@"; do
    echo "{\"check\": \"$check\", \"outcome\": \"pass\", \"message\": \"ok\"}" >> "$IGNITE_RESULTS"
  done ;;
esac
"#;

const MANIFEST: &str = r#"[family.slow]
kind = "regression"
adapter = "jsonl"
platforms = "any"
always = true
list = "sh checks/adapter.sh list"
run = "sh checks/adapter.sh run {checks}"
inputs = ["src/**"]
"#;

/// `ignite.toml` for a hub at `url` and this machine, `host`.
fn config(url: &str, host: &str) -> String {
    format!(
        r#"[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = ".trees"
checks = "checks"

[hub]
url = "{url}"
project = "restart"

[gate]
threshold = 1000000000.0

[machine.{host}]
ticket_trees = 2
landing_trees = 1
floor = {{ batch = 1, tip = 1 }}
"#
    )
}

/// A path as the adapter's shell reads it: forward slashes, also on Windows.
fn shell_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

struct Drill {
    root: PathBuf,
    remote: PathBuf,
    repo: PathBuf,
    home: PathBuf,
    gate: PathBuf,
    runs: PathBuf,
    hub: Option<RunningHub>,
    daemon: Option<Child>,
    /// The daemon's process ID after the restart; on Windows the new daemon is another process.
    restarted: Option<u32>,
}

impl Drop for Drill {
    fn drop(&mut self) {
        // Lets every waiting command finish, so no process outlives the test.
        let _ = fs::write(&self.gate, "");
        // A failed test can leave verb processes waiting on a daemon that is gone; they run
        // outside the daemon's process group, so ending the daemon does not end them.
        for pid in self.verb_processes() {
            kill(pid);
        }
        if let Some(pid) = self.restarted {
            kill(pid);
        }
        if let Some(mut daemon) = self.daemon.take() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
        if let Some(hub) = self.hub.take() {
            hub.stop();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Drill {
    /// The process IDs the daemon's handles record for their verb processes.
    fn verb_processes(&self) -> Vec<u32> {
        let Ok(db) = ignite_store::local::open_read_only(&self.home.join("daemon.db")) else {
            return Vec::new();
        };
        let Ok(mut statement) = db.prepare("SELECT pid FROM handles WHERE pid IS NOT NULL") else {
            return Vec::new();
        };
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map(|rows| {
                rows.filter_map(Result::ok)
                    .filter_map(|pid| u32::try_from(pid).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(unix)]
fn kill(pid: u32) {
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

#[cfg(windows)]
fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

impl Drill {
    /// The repository, the remote and the hub, under a short temporary path so the daemon's
    /// socket path stays short.
    fn new() -> Drill {
        let root = ignite_core::test_files::temp_dir().join(format!("irst-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("marks")).unwrap();
        let host = NativeHost.hostname();
        let placeholder = Config::parse("ignite.toml", &config("http://127.0.0.1:1", &host))
            .unwrap_or_else(|error| panic!("this machine's name {host:?}: {error}"));
        let hub = HubServer {
            config: placeholder,
            listen: "127.0.0.1:0".to_owned(),
            database: root.join("hub.db"),
            wordmark: None,
        }
        .start()
        .unwrap();
        let url = format!("http://{}", hub.address());
        let gate = root.join("gate");
        let adapter = ADAPTER
            .replace("@MARKS@", &shell_path(&root.join("marks")))
            .replace("@GATE@", &shell_path(&gate));
        let seed = root.join("seed");
        for (relative, text) in [
            ("ignite.toml", config(&url, &host)),
            (".gitignore", ".trees/\n".to_owned()),
            ("src/a.txt", "a\n".to_owned()),
            ("src/b.txt", "b\n".to_owned()),
            ("checks/families.toml", MANIFEST.to_owned()),
            ("checks/adapter.sh", adapter),
        ] {
            let path = seed.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let remote = root.join("origin.git");
        git(
            &root,
            &["init", "-q", "--bare", "-b", "staging", "origin.git"],
        );
        // Every update of a branch is logged, so the test can count the landing's pushes.
        git(&remote, &["config", "core.logAllRefUpdates", "always"]);
        git(&seed, &["init", "-q", "-b", "staging"]);
        identify(&seed);
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-q", "-m", "seed"]);
        git(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            &seed,
            &["push", "-q", "origin", "staging", "staging:master"],
        );
        git(
            &root,
            &[
                "clone",
                "-q",
                "-b",
                "staging",
                remote.to_str().unwrap(),
                "main",
            ],
        );
        let repo = root.join("main");
        identify(&repo);
        git(&repo, &["branch", "-q", "master", "origin/master"]);
        Drill {
            runs: root.join("marks").join("runs.log"),
            home: root.join("home"),
            remote,
            repo,
            gate,
            hub: Some(hub),
            daemon: None,
            restarted: None,
            root,
        }
    }

    /// `ignite` with this drill's home, as `session` (none for `None`).
    fn ignite(&self, cwd: &Path, session: Option<&str>, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ignite"));
        command
            .args(args)
            .current_dir(cwd)
            .env("IGNITE_HOME", &self.home)
            .env_remove("IGNITE_SESSION")
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID")
            .env_remove("IGNITE_HUB_LISTEN")
            .env_remove("IGNITE_HUB_PUBLIC_LISTEN")
            .env_remove("IGNITE_RESTART_FROM");
        if let Some(session) = session {
            command.env("IGNITE_SESSION", session);
        }
        command
    }

    /// Runs `ignite` to the end; its exit code and standard output.
    fn run(&self, cwd: &Path, session: Option<&str>, args: &[&str]) -> (Option<i32>, String) {
        let output = self.ignite(cwd, session, args).output().unwrap();
        (output.status.code(), text(&output))
    }

    /// Starts `ignite daemon` in the main checkout, logging to `daemon.log`, and waits until it
    /// answers and has created its trees. A new daemon creates its trees on a thread of its own
    /// after it starts answering, and an `ignite start` that finds no free tree meanwhile is
    /// told to wait (exit code 5) rather than given one.
    fn start_daemon(&mut self) {
        let log = fs::File::create(self.root.join("daemon.log")).unwrap();
        let daemon = self
            .ignite(&self.repo, None, &["daemon"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.daemon = Some(daemon);
        until("the daemon answers", || {
            self.run(&self.root, None, &["status"]).0 == Some(0)
        });
        until(
            "the daemon's two ticket trees and landing tree are free",
            || self.free_trees() == 3,
        );
    }

    /// How many of the daemon's trees are free. The database is opened read-only: a migrating
    /// open would race the starting daemon's own first migration.
    fn free_trees(&self) -> i64 {
        let Ok(db) = ignite_store::local::open_read_only(&self.home.join("daemon.db")) else {
            return 0;
        };
        db.query_row(
            "SELECT count(*) FROM trees WHERE state = 'free'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    /// `ignite start "<summary>"` for `session`: files the ticket, leases a tree and returns
    /// its path.
    fn start(&self, session: &str, summary: &str) -> PathBuf {
        let (code, out) = self.run(&self.repo, Some(session), &["--json", "start", summary]);
        assert_eq!(code, Some(0), "{out}");
        let started: serde_json::Value = serde_json::from_str(&out).unwrap();
        PathBuf::from(started["path"].as_str().unwrap())
    }

    /// The lines of `runs.log` that a test or a landing wrote, leaving out idle work.
    fn runs(&self) -> Vec<String> {
        fs::read_to_string(&self.runs)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.starts_with("idle "))
            .map(str::to_owned)
            .collect()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.root.join("daemon.log")).unwrap_or_default()
    }
}

/// Waits until `done` holds, failing the test after [`HUNG`].
fn until(what: &str, mut done: impl FnMut() -> bool) {
    let since = Instant::now();
    while !done() {
        assert!(since.elapsed() < HUNG, "hung waiting until {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits for `child` to exit, failing the test after [`HUNG`].
fn finish(what: &str, mut child: Child) -> (Option<i32>, String) {
    until(what, || child.try_wait().unwrap().is_some());
    let output = child.wait_with_output().unwrap();
    (output.status.code(), text(&output))
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn git(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn identify(repo: &Path) {
    git(repo, &["config", "user.name", "Ignite Test"]);
    git(repo, &["config", "user.email", "test@example.invalid"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

#[test]
fn a_restart_under_load_ends_no_command_and_runs_nothing_twice() {
    let mut drill = Drill::new();
    drill.start_daemon();
    let tester = drill.start("local_rsta", "Restart drill: a test");
    let lander = drill.start("local_rstb", "Restart drill: a landing");
    // A session that holds a tree, or a client inside one, may not restart the daemon.
    let (code, out) = drill.run(&drill.root, Some("local_rsta"), &["internal", "restart"]);
    assert_eq!(code, Some(3), "{out}");
    let (code, out) = drill.run(&tester, None, &["internal", "restart"]);
    assert_eq!(code, Some(3), "{out}");
    fs::write(lander.join("src/b.txt"), "b, changed\n").unwrap();
    git(&lander, &["commit", "-q", "-am", "feat: change b"]);

    let mut test = drill
        .ignite(&tester, Some("local_rsta"), &["test", "slow:one"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut land = drill
        .ignite(
            &lander,
            Some("local_rstb"),
            &["land", "-m", "Lands across a restart."],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    // The test's check and the gate's two checks are all running, waiting on the gate file.
    until("all three check commands run", || drill.runs().len() >= 3);

    let (code, out) = drill.run(&drill.root, None, &["--json", "internal", "restart"]);
    assert_eq!(code, Some(0), "{out}\n{}", drill.log());
    let restarted: serde_json::Value = serde_json::from_str(&out).unwrap();
    let pid = restarted["pid"].as_u64().expect("the new daemon's pid");
    drill.restarted = u32::try_from(pid).ok();
    assert!(drill.log().contains("restarting into"), "{}", drill.log());

    // Both clients outlived the daemon they asked, and both verb processes go on with the new
    // daemon.
    assert!(test.try_wait().unwrap().is_none());
    assert!(land.try_wait().unwrap().is_none());
    // The daemon's own tip round may be a verb process too, so look for each verb by name.
    until("the new daemon serves both verbs", || {
        let log = drill.log();
        ["`ignite test`", "`ignite land`"].iter().all(|verb| {
            log.lines()
                .any(|line| line.contains(verb) && line.contains("still runs in its verb process"))
        })
    });

    fs::write(&drill.gate, "").unwrap();
    let (code, out) = finish("`ignite test` ends", test);
    assert_eq!(code, Some(0), "{out}\n{}", drill.log());
    assert!(out.contains("all passed"), "{out}");
    let (code, out) = finish("`ignite land` ends", land);
    assert_eq!(code, Some(0), "{out}\n{}", drill.log());
    assert!(out.contains("landed"), "{out}");

    // Every check command ran once: none was ended and started again.
    let mut runs = drill.runs();
    runs.sort();
    let before = runs.len();
    runs.dedup();
    assert_eq!(before, runs.len(), "a check ran twice: {runs:?}");
    assert_eq!(runs.len(), 3, "{runs:?}");

    // One result per check and input hash: the test's `slow:one` on the seed, and the gate's
    // two checks on the change.
    let db = ignite_store::local::open_read_only(&drill.home.join("daemon.db")).unwrap();
    let mut statement = db
        .prepare("SELECT check_name, count(*) FROM results GROUP BY check_name ORDER BY 1")
        .unwrap();
    let results: Vec<(String, i64)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        results,
        [("slow:one".to_owned(), 2), ("slow:two".to_owned(), 1)]
    );

    // The landing pushed once: the remote's `staging` moved twice, by the seed and the landing.
    let moves = git(&drill.remote, &["reflog", "show", "--format=%H", "staging"]);
    assert_eq!(text(&moves).lines().count(), 2, "{}", text(&moves));
    let subjects = git(&drill.remote, &["log", "--format=%s", "staging"]);
    assert_eq!(
        text(&subjects).lines().collect::<Vec<_>>(),
        ["feat: change b", "seed"]
    );

    let log = drill.log();
    for cut in ["its tree is reset", "again after the restart"] {
        assert!(!log.contains(cut), "{cut:?} in the daemon's log:\n{log}");
    }
}

/// A fault file makes a starting daemon exit with code 1, as a build that crashes at start
/// would, before it takes the machine's lock or serves anything.
#[test]
fn a_fault_file_makes_the_daemon_exit_at_start() {
    let root = ignite_core::test_files::temp_dir().join(format!("irst-fault-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // An empty file names every build, including this one, which is in no store.
    fs::write(root.join("fault-start"), "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ignite"))
        .arg("daemon")
        .current_dir(&root)
        .env("IGNITE_HOME", &root)
        .env_remove("IGNITE_HUB_LISTEN")
        .env_remove("IGNITE_RESTART_FROM")
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(1), "{error}");
    assert!(error.contains("fault-start"), "{error}");
    assert!(!root.join("daemon.sock").exists());
    let _ = fs::remove_dir_all(&root);
}
