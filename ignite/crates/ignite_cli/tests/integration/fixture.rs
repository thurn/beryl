//! Child processes for the integration tests. Running this test binary again gives the
//! supervisor a native command on both operating systems.

use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ignite_core::protocol::Class;
use ignite_core::traits::CommandSpec;
use serde_json::json;

const ROLE: &str = "IGNITE_TEST_FIXTURE";
const ARGS: &[&str] = &["--exact", "fixture::child", "--ignored", "--nocapture"];

pub fn command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(ARGS).env(ROLE, role);
    command
}

pub fn spec(cwd: &Path, role: &str) -> CommandSpec {
    let command = command(role);
    CommandSpec {
        args: std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect(),
        cwd: cwd.to_path_buf(),
        env: vec![(ROLE.to_owned(), role.to_owned())],
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    }
}

pub fn waiting(cwd: &Path, gate: &Path, exit: i32) -> CommandSpec {
    let mut spec = spec(cwd, "wait");
    spec.env.extend([
        ("FIXTURE_GATE".to_owned(), gate.to_str().unwrap().to_owned()),
        ("FIXTURE_EXIT".to_owned(), exit.to_string()),
    ]);
    spec
}

/// A connection held by the grandchild for its whole lifetime lets the test observe its exit.
pub fn with_descendant(cwd: &Path, waits: bool) -> (CommandSpec, TcpListener) {
    let role = if waits {
        "wait-with-descendant"
    } else {
        "leave-descendant"
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut spec = spec(cwd, role);
    spec.env.extend([
        (
            "FIXTURE_LISTENER".to_owned(),
            listener.local_addr().unwrap().to_string(),
        ),
        (
            "FIXTURE_GATE".to_owned(),
            cwd.join("never").to_str().unwrap().to_owned(),
        ),
    ]);
    (spec, listener)
}

pub fn assert_disconnected(listener: &TcpListener) {
    let (mut stream, _) = listener.accept().expect("the grandchild connected");
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    match stream.read(&mut [0]) {
        Ok(0) => {}
        Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
        other => panic!("the grandchild's connection stayed open: {other:?}"),
    }
}

/// The manifest splits commands on whitespace. A relative script lets it start the native
/// fixture executable even when the executable's absolute path contains spaces.
pub fn adapter(role: &str) -> (&'static str, &'static str, String) {
    let executable = ignite_os::paths::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let executable = executable.to_str().unwrap();
    let args = ARGS.join(" ");
    if cfg!(windows) {
        let executable = executable.replace('%', "%%");
        (
            "checks/adapter.cmd",
            r"cmd /d /c checks\adapter.cmd",
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\nset \"{ROLE}={role}\"\r\n\
                 set \"IGNITE_TEST_CHECK_ARGS=%*\"\r\n\"{executable}\" {args}\r\n"
            ),
        )
    } else {
        let executable = executable.replace('\'', "'\\''");
        (
            "checks/adapter.sh",
            "sh checks/adapter.sh",
            format!("{ROLE}={role} IGNITE_TEST_CHECK_ARGS=\"$*\" exec '{executable}' {args}\n"),
        )
    }
}

/// Give up if a failed test never opens its gate, so the child cannot outlive the test run
/// indefinitely.
pub fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_mins(10);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "fixture condition did not become true"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap()
}

fn gate() {
    let path = PathBuf::from(var("FIXTURE_GATE"));
    until(|| path.exists());
}

/// Libtest writes the test name before calling the fixture. Start any response on a new line.
fn output(message: impl std::fmt::Display) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "\n{message}").unwrap();
    stdout.flush().unwrap();
}

#[test]
#[ignore = "run as a child process by the integration tests"]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let code = match role.as_str() {
        "wait" => {
            fs::write("started", "").unwrap();
            gate();
            output("finished");
            var("FIXTURE_EXIT").parse().unwrap()
        }
        "report" => {
            let cwd = ignite_os::paths::canonicalize(".").unwrap();
            output(format_args!(
                "{} from {} with {} cores",
                var("GREETING"),
                cwd.display(),
                var("IGNITE_CORES")
            ));
            writeln!(io::stderr(), "oops").unwrap();
            assert!(command("compute").status().unwrap().success());
            7
        }
        "compute" => {
            for value in 0..5_000_000_u64 {
                std::hint::black_box(value.wrapping_mul(value));
            }
            0
        }
        "descendant" => {
            let _connection = TcpStream::connect(var("FIXTURE_LISTENER")).unwrap();
            fs::write("descendant-started", "").unwrap();
            gate();
            0
        }
        "leave-descendant" | "wait-with-descendant" => {
            // The background-process test leaves the child for the supervisor to end.
            #[allow(clippy::zombie_processes)]
            let mut child = command("descendant").stdout(Stdio::null()).spawn().unwrap();
            if role == "wait-with-descendant" {
                assert!(child.wait().unwrap().success());
            } else {
                until(|| Path::new("descendant-started").exists());
            }
            0
        }
        "watch-cores" => {
            let path = PathBuf::from(var("IGNITE_CORES_FILE"));
            let cores = || fs::read_to_string(&path).unwrap_or_default();
            let first = cores();
            output(format_args!("first {}", first.trim()));
            fs::write("started", "").unwrap();
            until(|| cores().trim() == "4");
            output("last 4");
            0
        }
        "core-checks" | "slow-checks" => {
            checks(&role);
            0
        }
        _ => panic!("unknown fixture role {role}"),
    };
    std::process::exit(code);
}

fn checks(role: &str) {
    let args = var("IGNITE_TEST_CHECK_ARGS");
    let mut args = args.split_whitespace();
    let core = role == "core-checks";
    match args.next().unwrap() {
        "list" => {
            let names: &[_] = if core {
                &["core:alpha_adds"]
            } else {
                &["slow:one", "slow:two"]
            };
            for check in names {
                output(json!({ "check": check }));
            }
        }
        "run" => {
            let checks: Vec<_> = args.collect();
            let (outcome, message) = if core {
                let source = "crates/alpha/src/lib.rs";
                let text = fs::read_to_string(source).unwrap();
                let outcome = if text.contains("BROKEN") {
                    "fail"
                } else {
                    "pass"
                };
                (outcome, source)
            } else {
                let source = fs::read_to_string("src/b.txt").unwrap();
                let mut log = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(var("FIXTURE_RUNS"))
                    .unwrap();
                let class = var("IGNITE_CLASS");
                for check in &checks {
                    let line = format!("{class} {check} {}\n", source.trim_end());
                    log.write_all(line.as_bytes()).unwrap();
                }
                gate();
                ("pass", "ok")
            };
            let mut results = OpenOptions::new()
                .create(true)
                .append(true)
                .open(var("IGNITE_RESULTS"))
                .unwrap();
            for check in checks {
                writeln!(
                    results,
                    "{}",
                    json!({ "check": check, "outcome": outcome, "message": message })
                )
                .unwrap();
            }
        }
        action => panic!("unknown adapter action {action}"),
    }
}
