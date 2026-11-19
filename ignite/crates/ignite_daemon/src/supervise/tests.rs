//! `ignite run`'s arguments and reply, against the fake launcher. The supervisor itself is
//! tested through the `ignite` executable in `ignite_cli`'s `tests/integration/supervise.rs`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::protocol::Usage;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::MachineId;
use ignite_core::time::Timestamp;

use super::*;

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn context(launcher: Arc<FakeLauncher>) -> Context {
    let text = r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.local]
        ticket_trees = 1
        landing_trees = 1
    "#;
    Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher,
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn request(args: &[&str]) -> Request {
    Request {
        verb: "run".to_owned(),
        args: strings(args),
        cwd: PathBuf::from("/trees/tree-01"),
        session: None,
        wait_s: 540,
        pid: None,
    }
}

#[test]
fn run_args_take_class_ticket_and_cores_before_the_command() {
    let args = parse_run_args(&strings(&[
        "--class", "batch", "--ticket", "Tix1t.12", "--cores", "4", "--", "just", "--games", "2000",
    ]))
    .unwrap();
    assert_eq!(args.class, Class::Batch);
    assert_eq!(args.ticket.unwrap().as_str(), "Tix1t.12");
    assert_eq!(args.cores, Some(4));
    assert_eq!(args.command, strings(&["just", "--games", "2000"]));
    assert!(!args.grows);
    assert!(
        parse_run_args(&strings(&["--grows", "--", "record"]))
            .unwrap()
            .grows
    );

    let plain = parse_run_args(&strings(&["--", "cargo", "build"])).unwrap();
    assert_eq!(
        (plain.class, plain.ticket, plain.cores),
        (Class::Agent, None, None)
    );
}

#[test]
fn run_args_without_a_command_or_with_a_bad_class_are_usage_errors() {
    assert!(parse_run_args(&strings(&["--class", "batch"])).is_err());
    assert!(parse_run_args(&strings(&["--class", "urgent", "--", "x"])).is_err());
    assert!(parse_run_args(&strings(&["--cores", "many", "--", "x"])).is_err());
    assert!(parse_run_args(&strings(&["cargo", "build"])).is_err());
}

#[test]
fn the_run_verb_hands_the_command_to_the_launcher_in_the_callers_directory() {
    let launcher = Arc::new(FakeLauncher::default());
    let ctx = context(Arc::clone(&launcher));
    let reply = RunVerb.run(
        &ctx,
        &request(&[
            "--class", "batch", "--ticket", "Tix1t.12", "--", "just", "x",
        ]),
    );
    assert_eq!(reply.exit, Exit::Success);
    let runs = launcher.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].args, strings(&["just", "x"]));
    assert_eq!(runs[0].cwd, PathBuf::from("/trees/tree-01"));
    assert_eq!(runs[0].class, Class::Batch);
    assert_eq!(runs[0].ticket.as_ref().unwrap().as_str(), "Tix1t.12");
    assert!(runs[0].cores >= 1);
}

#[test]
fn a_failed_command_replies_check_failed_with_its_exit_and_the_end_of_its_output() {
    let dir = std::env::temp_dir().join(format!("ignite-run-reply-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output = dir.join("output.log");
    let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
    fs::write(&output, lines.join("\n")).unwrap();
    let launcher = Arc::new(FakeLauncher::default());
    launcher.script(
        &["cargo"],
        RunResult {
            exit: Some(101),
            output: output.clone(),
            usage: Usage {
                wall_s: 12.4,
                ..Usage::default()
            },
            hung: false,
        },
    );
    let reply = RunVerb.run(&context(launcher), &request(&["--", "cargo", "test"]));
    fs::remove_dir_all(&dir).unwrap();

    assert_eq!(reply.exit, Exit::CheckFailed);
    let text: Vec<&str> = reply.text.lines().collect();
    assert_eq!(text[0], "failed   cargo test in 12 s: exit 101");
    assert_eq!(text[1], "line 71");
    assert_eq!(text[TAIL_LINES], "line 100");
    assert_eq!(
        text.last().unwrap(),
        &format!("output   {}", output.display())
    );
    assert!(text.len() <= 40);
    let json = reply.json.unwrap();
    assert_eq!(json["exit"], 101);
}

#[test]
fn a_run_has_a_hang_limit_unless_asked_for_another() {
    let launcher = Arc::new(FakeLauncher::default());
    let ctx = context(Arc::clone(&launcher));
    RunVerb.run(&ctx, &request(&["--", "cargo", "test"]));
    RunVerb.run(
        &ctx,
        &request(&["--hang-limit", "60", "--", "cargo", "test"]),
    );
    let limits: Vec<Option<u64>> = launcher.runs().iter().map(|r| r.hang_limit_s).collect();
    assert_eq!(limits, [Some(DEFAULT_HANG_LIMIT_S), Some(60)]);
    assert!(parse_run_args(&strings(&["--hang-limit", "0", "--", "x"])).is_err());
    assert!(parse_run_args(&strings(&["--hang-limit", "soon", "--", "x"])).is_err());
}

#[test]
fn a_command_ended_as_hung_is_reported_as_failed() {
    let result = RunResult {
        exit: None,
        output: PathBuf::from("/runs/1/output.log"),
        usage: Usage::default(),
        hung: true,
    };
    let reply = render(&strings(&["cargo", "test"]), &result);
    assert_eq!(reply.exit, Exit::CheckFailed);
    assert!(
        reply.text.starts_with("failed   cargo test in 0 s: hung"),
        "{}",
        reply.text
    );
}

#[test]
fn a_run_in_the_express_directory_of_a_tree_names_ignite_test_of_the_express_families() {
    let mut ctx = context(Arc::new(FakeLauncher::default()));
    ctx.config.express = Some(ignite_core::config::Express {
        paths: strings(&["tool/**"]),
        tree: PathBuf::from("trees/express"),
        dir: "tool".to_owned(),
        commands: strings(&["cargo test"]),
        families: strings(&["tool", "tool-lint"]),
    });
    let run_in = |cwd: &str| {
        let mut req = request(&["--", "cargo", "test"]);
        req.cwd = PathBuf::from(cwd);
        RunVerb.run(&ctx, &req).text
    };
    let hinted = run_in("/repo/trees/tree-01/tool/crates");
    assert!(
        hinted.contains("hint     `ignite test tool: tool-lint:` runs"),
        "{hinted}"
    );
    assert!(!run_in("/repo/trees/tree-01/rust").contains("hint"));
    assert!(!run_in("/repo/tool").contains("hint"), "the main checkout");
}
