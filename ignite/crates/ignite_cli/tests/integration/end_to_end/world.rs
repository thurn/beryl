//! The world the end-to-end tests run in: a bare repository standing in for the remote, a real
//! hub on a random port, and daemons assembled in the test's process, each with its own clone
//! of the remote and its own home folder.
//!
//! The repository holds a two-crate Cargo workspace and a manifest of two `jsonl` families,
//! whose shell adapter fails a check when its crate's source contains the word `BROKEN`:
//!
//! - `core` checks `alpha`. It is marked `always`, so every landing that touches `alpha` runs it.
//! - `extra` checks `beta`. The gate threshold is set so high that the gate never chooses it,
//!   so only tip verification runs it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use ignite_core::config::Config;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Reply, Request};
use ignite_core::protocol::tickets::{BatchRequest, Operation, TicketDetail, TicketFields};
use ignite_daemon::main::{Daemon, DaemonOptions};
use ignite_daemon::server::serve_listener;
use ignite_daemon::update::Home;
use ignite_hub::server::{HubServer, RunningHub};
use ignite_os::ipc::Ipc;

/// The checks' adapter: `list <family>` prints the family's checks, `run <checks>…` writes one
/// result line per check to `$IGNITE_RESULTS`.
const ADAPTER: &str = r#"#!/bin/sh
case "$1" in
list)
  case "$2" in
  core) echo '{"check": "core:alpha_adds"}' ;;
  extra) echo '{"check": "extra:beta_adds"}' ;;
  esac ;;
run)
  shift
  for check in "$@"; do
    case "$check" in
    core:alpha_adds) source=crates/alpha/src/lib.rs ;;
    extra:beta_adds) source=crates/beta/src/lib.rs ;;
    esac
    if grep -q BROKEN "$source"; then outcome=fail; else outcome=pass; fi
    echo "{\"check\": \"$check\", \"outcome\": \"$outcome\", \"message\": \"$source\"}" >> "$IGNITE_RESULTS"
  done ;;
esac
"#;

const MANIFEST: &str = r#"[family.core]
kind = "regression"
adapter = "jsonl"
platforms = "any"
always = true
list = "sh checks/adapter.sh list core"
run = "sh checks/adapter.sh run {checks}"
inputs = ["crate:alpha"]

[family.extra]
kind = "regression"
adapter = "jsonl"
platforms = "any"
list = "sh checks/adapter.sh list extra"
run = "sh checks/adapter.sh run {checks}"
inputs = ["crate:beta"]
"#;

/// `ignite.toml` for a hub at `url`, with two machines.
fn config(url: &str) -> String {
    format!(
        r#"[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = ".trees"
checks = "checks"

[hub]
url = "{url}"
project = "e2e"

[gate]
threshold = 1000000000.0
cap = 400

[machine.alpha]
ticket_trees = 2
landing_trees = 2
floor = {{ batch = 1, tip = 1 }}

[machine.beta]
ticket_trees = 2
landing_trees = 2
floor = {{ batch = 1, tip = 1 }}
"#
    )
}

pub struct World {
    pub root: PathBuf,
    pub remote: PathBuf,
    pub hub: Option<RunningHub>,
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(hub) = self.hub.take() {
            hub.stop();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl World {
    /// A fresh world under a short temporary path, so the daemons' socket paths stay short.
    pub fn new(name: &str) -> World {
        let root = ignite_core::test_files::temp_dir().join(format!("ie2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let placeholder = Config::parse("ignite.toml", &config("http://127.0.0.1:1")).unwrap();
        let hub = HubServer {
            config: placeholder,
            listen: "127.0.0.1:0".to_owned(),
            database: root.join("hub.db"),
            wordmark: None,
        }
        .start()
        .unwrap();
        let url = format!("http://{}", hub.address());
        let remote = root.join("origin.git");
        git(
            &root,
            &["init", "-q", "--bare", "-b", "staging", "origin.git"],
        );
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        write_repository(&seed, &url);
        git(&seed, &["init", "-q", "-b", "staging"]);
        identify(&seed);
        run(
            &seed,
            "cargo",
            &["generate-lockfile", "--offline", "--quiet"],
        );
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
        World {
            root,
            remote,
            hub: Some(hub),
        }
    }

    /// A clone of the remote for one machine's daemon.
    pub fn checkout(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        git(
            &self.root,
            &[
                "clone",
                "-q",
                "-b",
                "staging",
                self.remote.to_str().unwrap(),
                name,
            ],
        );
        identify(&path);
        git(&path, &["branch", "-q", "master", "origin/master"]);
        path
    }

    /// A daemon for `machine`, declared in `ignite.toml`, over its own clone.
    pub fn daemon(&self, machine: &str) -> Arc<Daemon> {
        let repo = self.checkout(machine);
        let home = Home {
            root: self.root.join(format!("home-{machine}")),
        };
        let daemon = Daemon::start(DaemonOptions {
            repo,
            home,
            hostname: machine.to_owned(),
            ignite: PathBuf::from(env!("CARGO_BIN_EXE_ignite")),
            version: "e2e".to_owned(),
            feed_wait_s: 0,
        })
        .unwrap_or_else(|error| panic!("the daemon for {machine} did not start: {error}"));
        // Its socket is served even for a test that asks the server directly: the long verbs
        // run in verb processes, which reach the daemon through it.
        let socket = daemon.options.home.root.join("daemon.sock");
        let listener = ignite_os::Native.listen(&socket.to_string_lossy()).unwrap();
        let server = Arc::clone(&daemon.server);
        std::thread::spawn(move || serve_listener(&server, listener.as_ref()));
        Arc::new(daemon)
    }

    /// The remote's newest commit on `branch`.
    pub fn remote_tip(&self, branch: &str) -> String {
        let out = git(&self.remote, &["rev-parse", branch]);
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// The subjects of the remote branch's commits, newest first.
    pub fn subjects(&self, branch: &str) -> Vec<String> {
        let out = git(&self.remote, &["log", "--format=%s", branch]);
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// A new open ticket at the hub.
pub fn ticket(daemon: &Daemon, title: &str) -> TicketId {
    let reply = daemon
        .ctx
        .hub
        .batch(&BatchRequest {
            preconditions: Vec::new(),
            ops: vec![Operation::Create {
                fields: TicketFields {
                    title: Some(title.to_owned()),
                    ..TicketFields::default()
                },
                parent: None,
                deps: Vec::new(),
                request_key: None,
            }],
        })
        .unwrap();
    reply.results[0].id.clone()
}

pub fn detail(daemon: &Daemon, ticket: &TicketId) -> TicketDetail {
    daemon.ctx.hub.ticket(ticket).unwrap()
}

/// Runs `verb` in the daemon from `cwd` for `session`, as the client would send it.
pub fn ask(daemon: &Daemon, session: &str, cwd: &Path, verb: &str, args: &[&str]) -> Reply {
    daemon.server.handle(&Request {
        verb: verb.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: cwd.to_path_buf(),
        session: Some(SessionId::new(session).unwrap()),
        wait_s: 600,
        pid: None,
    })
}

/// `ignite start <ticket>` for `session`; returns the tree's path.
pub fn start(daemon: &Daemon, session: &str, ticket: &TicketId) -> PathBuf {
    let reply = ask(
        daemon,
        session,
        &daemon.options.repo,
        "start",
        &[ticket.as_str()],
    );
    assert_eq!(reply.exit.code(), 0, "start failed: {}", reply.text);
    let json = reply.json.expect("start answers with JSON");
    PathBuf::from(json["path"].as_str().unwrap())
}

/// Appends `text` to the file at `relative` in `tree` and commits it.
pub fn commit(tree: &Path, relative: &str, text: &str, subject: &str) {
    let path = tree.join(relative);
    let mut contents = std::fs::read_to_string(&path).unwrap();
    contents.push_str(text);
    std::fs::write(&path, contents).unwrap();
    git(tree, &["commit", "-q", "-am", subject]);
}

/// Runs `git` in `dir` and fails the test if it fails.
pub fn git(dir: &Path, args: &[&str]) -> Output {
    run(dir, "git", args)
}

fn run(dir: &Path, program: &str, args: &[&str]) -> Output {
    let out = Command::new(program)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Gives a clone an author and turns off signing, which the machine's own settings may ask for.
fn identify(repo: &Path) {
    git(repo, &["config", "user.name", "Ignite Test"]);
    git(repo, &["config", "user.email", "test@example.invalid"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

fn write_repository(dir: &Path, url: &str) {
    let files: &[(&str, String)] = &[
        ("ignite.toml", config(url)),
        (".gitignore", ".trees/\ntarget/\n".to_owned()),
        (
            "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/alpha\", \"crates/beta\"]\n"
                .to_owned(),
        ),
        (
            "crates/alpha/Cargo.toml",
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_owned(),
        ),
        (
            "crates/alpha/src/lib.rs",
            "pub fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n".to_owned(),
        ),
        (
            "crates/beta/Cargo.toml",
            "[package]\nname = \"beta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nalpha = { path = \"../alpha\" }\n"
                .to_owned(),
        ),
        (
            "crates/beta/src/lib.rs",
            "pub fn twice(a: u32) -> u32 {\n    alpha::add(a, a)\n}\n".to_owned(),
        ),
        ("checks/families.toml", MANIFEST.to_owned()),
        ("checks/adapter.sh", ADAPTER.to_owned()),
    ];
    for (relative, text) in files {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}
