//! The `ignite` executable, run as a user would run it.

use std::process::Command;

fn ignite(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(args)
        .env_remove("IGNITE_SESSION")
        .output()
        .unwrap()
}

#[test]
fn help_lists_the_agent_verbs() {
    let output = ignite(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for verb in [
        "start", "test", "check", "sync", "land", "run", "ticket", "why", "status",
    ] {
        assert!(help.contains(verb), "{verb} missing from {help}");
    }
}

#[test]
fn an_unbuilt_verb_names_its_ticket_and_exits_1() {
    let output = ignite(&["gate", "replay"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "not implemented: Gate replay\n"
    );
}

#[test]
fn a_usage_error_exits_nonzero() {
    let output = ignite(&["land"]);
    assert!(!output.status.success());
}

/// A git repository in the temporary directory with an `ignite.toml`, one tracked source file
/// and a manifest whose inputs are `inputs`.
fn lint_repository(name: &str, inputs: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-lint-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("checks")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("ignite.toml"),
        "[repository]\nremote = \"origin\"\nstaging = \"staging\"\nmaster = \"master\"\n\
         trees = \".trees\"\nchecks = \"checks\"\n\n[hub]\nurl = \"https://hub\"\n\
         project = \"p\"\n\n[gate]\nthreshold = 0.1\ncap = 10\n\n[machine.m]\nticket_trees = 1\nlanding_trees = 1\n\
         floor = { batch = 1, tip = 1 }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.py"), "").unwrap();
    std::fs::write(
        dir.join("checks/py.toml"),
        format!(
            "[family.py]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
             list = \"l\"\nrun = \"r\"\ninputs = {inputs}\n"
        ),
    )
    .unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    dir
}

fn lint_in(dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ignite"))
        .args(["manifest", "lint"])
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn manifest_lint_exits_1_on_a_pattern_that_matches_nothing() {
    let dir = lint_repository("bad", r#"["src/**", "lib/*.py"]"#);
    let output = lint_in(&dir);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "family.py.inputs[1]: lib/*.py: matches no tracked file\n"
    );
    let dir = lint_repository("good", r#"["src/**", "checks/*.toml"]"#);
    let output = lint_in(&dir);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    for name in ["bad", "good"] {
        let dir = std::env::temp_dir().join(format!("ignite-lint-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
