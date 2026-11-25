//! Pins version 1 of the host protocol (`ignite_core::protocol::host`).
//!
//! Each file in `tests/host_v1/` is the JSON of one request or reply body, or of the envelope
//! around one. The test checks that every file still parses into its type, serialises back to
//! exactly the same JSON, equals the value built here, and still parses with a field added, as
//! a newer peer may send. Version 1 is frozen: when this test fails, the change is wrong, not
//! the files. Add an optional field or a new call instead (see the module's documentation).
//!
//! Run with `IGNITE_WRITE_MISSING_GOLDEN=1` to write the file of a body that has none yet. It
//! never overwrites a file.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use ignite_core::ids::{Commit, Handle, SessionId, TicketId, TreeId};
use ignite_core::protocol::host::{
    Ack, AllowStop, Build, BuildReply, BuildWho, CALLS, Command, Finished, Hello, HelloReply,
    HostCall, LeaseLanding, LeaseLandingReply, Poll, RefuseStop, RefuseStopReply, ReleaseLanding,
    RequestId, Run, RunReply, StepUpdate, StopState, encode_reply, encode_request,
};
use ignite_core::protocol::hub::CompileUnit;
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::Error;

fn folder() -> PathBuf {
    let relative = "ignite/crates/ignite_core/tests/host_v1";
    // Opening the folder through the helper makes it an input of the tests that read it.
    ignite_core::test_files::read_dir(relative).unwrap();
    ignite_core::test_files::root().unwrap().join(relative)
}

fn handle() -> Handle {
    Handle::new("L-6").unwrap()
}

fn commit() -> Commit {
    Commit::new("87631b5").unwrap()
}

fn tree() -> TreeId {
    TreeId::new("landing-2").unwrap()
}

fn ticket() -> TicketId {
    TicketId::new("Tq44s.9.48").unwrap()
}

fn asked_at() -> Timestamp {
    Timestamp::parse("2026-11-18T16:10:38.000Z").unwrap()
}

fn usage() -> Usage {
    Usage {
        queued_s: 1.5,
        wall_s: 28.0,
        cpu_s: 112.25,
        instructions: Some(9_000_000_000),
        peak_mb: 812,
    }
}

/// Checks the golden file `name` against `sample`, and returns the file's name.
fn golden<T>(name: &str, sample: &T) -> String
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let file = format!("{name}.json");
    let path = folder().join(&file);
    let built = serde_json::to_value(sample).unwrap();
    if !path.exists() && std::env::var_os("IGNITE_WRITE_MISSING_GOLDEN").is_some() {
        let mut text = serde_json::to_string_pretty(&built).unwrap();
        text.push('\n');
        fs::write(&path, text).unwrap();
    }
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let pinned: Value = serde_json::from_str(&text).unwrap();
    let parsed: T = serde_json::from_value(pinned.clone())
        .unwrap_or_else(|e| panic!("{file} no longer parses: {e}"));
    assert_eq!(&parsed, sample, "{file} parses to another value");
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        pinned,
        "{file} serialises differently"
    );
    assert_eq!(
        built, pinned,
        "the value built for {file} serialises differently"
    );
    if let Value::Object(mut object) = pinned {
        object.insert("added_later".to_owned(), Value::from(1));
        let widened: T = serde_json::from_value(Value::Object(object))
            .unwrap_or_else(|e| panic!("{file} with an unknown field does not parse: {e}"));
        assert_eq!(&widened, sample, "an unknown field changed {file}");
    }
    file
}

/// The request and reply files of one call; `name` is the verb without its `host-` prefix.
fn call<C>(request: &C, replies: &[(&str, C::Reply)]) -> Vec<String>
where
    C: HostCall + PartialEq + Debug,
    C::Reply: PartialEq + Debug,
{
    let name = C::VERB.strip_prefix("host-").unwrap();
    let mut files = vec![golden(&format!("{name}.request"), request)];
    for (variant, reply) in replies {
        let file = if variant.is_empty() {
            format!("{name}.reply")
        } else {
            format!("{name}.reply.{variant}")
        };
        files.push(golden(&file, reply));
    }
    files
}

fn command() -> Command {
    Command {
        args: vec![
            "cargo".to_owned(),
            "nextest".to_owned(),
            "run".to_owned(),
            "--no-run".to_owned(),
        ],
        cwd: PathBuf::from("/trees/landing-2"),
        env: vec![("CARGO_TERM_COLOR".to_owned(), "never".to_owned())],
        class: Class::Land,
        cores: 8,
        resources: vec!["gpu".to_owned()],
        grows: true,
        tree: Some(tree()),
        ticket: Some(ticket()),
        hang_limit_s: Some(1800),
        reserve: None,
    }
}

fn hello_and_run_files() -> Vec<String> {
    let mut files = Vec::new();
    files.extend(call(
        &Hello {
            handle: handle(),
            protocol: 1,
            executable: "87631b5".to_owned(),
            pid: 48213,
        },
        &[(
            "",
            HelloReply {
                protocol: 1,
                running: true,
                stop: false,
                daemon_started: None,
            },
        )],
    ));
    files.extend(call(
        &Run {
            handle: handle(),
            request_id: RequestId::new(handle(), 17),
            first_asked_at: asked_at(),
            spec: command(),
        },
        &[
            (
                "waiting",
                RunReply::Waiting {
                    what: "8 cores".to_owned(),
                },
            ),
            (
                "started",
                RunReply::Started {
                    run: PathBuf::from("/home/.config/ignite/runs/20261118-161038-a1b2"),
                },
            ),
            (
                "refused",
                RunReply::Refused {
                    reason: "a stop was asked".to_owned(),
                },
            ),
        ],
    ));
    files
}

fn build_files() -> Vec<String> {
    let mut files = Vec::new();
    files.extend(call(
        &Build {
            handle: handle(),
            request_id: RequestId::new(handle(), 18),
            first_asked_at: asked_at(),
            tree: tree(),
            product: "rust-tests".to_owned(),
            packages: vec!["engine".to_owned()],
            who: BuildWho {
                class: Class::Land,
                ticket: Some(ticket()),
                session: Some(SessionId::new("local_c8470108").unwrap()),
                retry_failure: false,
            },
        },
        &[
            (
                "waiting",
                BuildReply::Waiting {
                    what: "building rust-tests in landing-2".to_owned(),
                },
            ),
            ("current", BuildReply::Current),
            (
                "built",
                BuildReply::Built {
                    usage: usage(),
                    units: vec![CompileUnit {
                        crate_name: "engine".to_owned(),
                        mode: "test".to_owned(),
                        instructions: Some(1.5e9),
                        cpu_s: 3.5,
                        rebuilt: true,
                    }],
                },
            ),
            (
                "failed",
                BuildReply::Failed {
                    output: PathBuf::from("/home/.config/ignite/runs/b/output.log"),
                    usage: usage(),
                },
            ),
        ],
    ));
    files
}

fn tree_files() -> Vec<String> {
    let mut files = Vec::new();
    files.extend(call(
        &LeaseLanding {
            handle: handle(),
            holder: "m-7f3a:Tq44s.9.48".to_owned(),
            commit: commit(),
            keep_free: 0,
        },
        &[
            (
                "leased",
                LeaseLandingReply::Leased {
                    tree: tree(),
                    path: PathBuf::from("/trees/landing-2"),
                    branch: "landing-2".to_owned(),
                    at: commit(),
                },
            ),
            ("busy", LeaseLandingReply::Busy),
        ],
    ));
    files.extend(call(
        &ReleaseLanding {
            handle: handle(),
            tree: tree(),
            built: Some(commit()),
        },
        &[("", Ack {})],
    ));
    files
}

fn step_and_stop_files() -> Vec<String> {
    let mut files = Vec::new();
    files.extend(call(
        &StepUpdate {
            handle: handle(),
            step: Some("running the gate".to_owned()),
            waiting: vec!["8 cores".to_owned()],
            class: Some(Class::Land),
            ticket: Some(ticket()),
        },
        &[(
            "",
            StopState {
                stop: false,
                daemon_started: None,
            },
        )],
    ));
    files.extend(call(
        &RefuseStop {
            handle: handle(),
            reason: "pushing to staging".to_owned(),
        },
        &[("", RefuseStopReply { refused: true })],
    ));
    files.extend(call(&AllowStop { handle: handle() }, &[("", Ack {})]));
    files.extend(call(
        &Poll { handle: handle() },
        &[(
            "",
            StopState {
                stop: true,
                daemon_started: None,
            },
        )],
    ));
    files.extend(call(&Finished { handle: handle() }, &[("", Ack {})]));

    files
}

fn envelope_files() -> Vec<String> {
    let mut files = Vec::new();
    let poll = Poll { handle: handle() };
    files.push(golden(
        "envelope.request",
        &encode_request(&poll, PathBuf::from("/trees/tree-07")),
    ));
    files.push(golden(
        "envelope.reply",
        &encode_reply(&Ok(StopState {
            stop: false,
            daemon_started: None,
        })),
    ));
    files.push(golden(
        "envelope.error",
        &encode_reply::<Ack>(&Err(Error::Conflict("landing-2 is not landing".to_owned()))),
    ));

    files
}

#[test]
fn version_1_is_unchanged() {
    let named: BTreeSet<String> = [
        hello_and_run_files(),
        build_files(),
        tree_files(),
        step_and_stop_files(),
        envelope_files(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let present: BTreeSet<String> = fs::read_dir(folder())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        present, named,
        "every golden file is checked, and only those"
    );
    let verbs: BTreeSet<&str> = CALLS
        .iter()
        .filter(|(_, since)| *since == 1)
        .map(|(verb, _)| *verb)
        .collect();
    let pinned: BTreeSet<&str> = named
        .iter()
        .filter_map(|file| file.strip_suffix(".request.json"))
        .filter(|name| *name != "envelope")
        .map(|name| {
            CALLS
                .iter()
                .filter(|(_, since)| *since == 1)
                .find(|(verb, _)| verb.strip_prefix("host-") == Some(name))
                .map(|(verb, _)| *verb)
                .unwrap()
        })
        .collect();
    assert_eq!(pinned, verbs, "every version 1 call has a golden request");
}
