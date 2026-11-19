//! A real Cargo build through the compiler wrapper and the test runner of the `ignite`
//! executable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::protocol::hub::CompileUnit;
use ignite_daemon::wrap::{RUN_MODE, cargo_env, write_shims};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-wrap-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A workspace of two independent library crates, `alpha` and `beta`, each with one test.
fn fixture(root: &Path) -> PathBuf {
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = [\"alpha\", \"beta\"]\n",
    )
    .unwrap();
    for name in ["alpha", "beta"] {
        let src = workspace.join(name).join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            workspace.join(name).join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        )
        .unwrap();
        fs::write(
            src.join("lib.rs"),
            "pub fn one() -> u32 { 1 }\n#[test]\nfn works() { assert_eq!(one(), 1); }\n",
        )
        .unwrap();
    }
    workspace
}

fn cargo(workspace: &Path, shims: &Path, units: &Path, args: &[&str]) {
    let mut command = Command::new(env!("CARGO"));
    command
        .args(args)
        .arg("--quiet")
        .current_dir(workspace)
        .env("CARGO_TARGET_DIR", workspace.join("target"))
        .env("CARGO_INCREMENTAL", "1");
    // Settings of the Cargo running this test must not reach the fixture's build.
    for name in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_BUILD_RUSTC_WRAPPER",
    ] {
        command.env_remove(name);
    }
    command.envs(cargo_env(shims, units));
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "cargo {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_units(path: &Path) -> Vec<CompileUnit> {
    let units = fs::read_to_string(path).unwrap_or_default();
    let _ = fs::remove_file(path);
    units
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_build_through_the_wrapper_records_each_unit_and_only_the_touched_one_again() {
    let root = scratch("build");
    let workspace = fixture(&root);
    let shims = root.join("shims");
    write_shims(&shims, Path::new(env!("CARGO_BIN_EXE_ignite"))).unwrap();
    let units = root.join("units.jsonl");

    cargo(&workspace, &shims, &units, &["build"]);
    let mut first = read_units(&units);
    first.sort_by(|a, b| a.crate_name.cmp(&b.crate_name));
    let names: Vec<_> = first.iter().map(|u| u.crate_name.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    for unit in &first {
        assert_eq!(unit.mode, "build");
        assert!(!unit.rebuilt, "{unit:?}");
        assert!(unit.cpu_s > 0.0);
        // Windows counts no instructions, only CPU time.
        assert!(cfg!(windows) || unit.instructions.unwrap_or(0.0) > 0.0);
    }

    // Touch one crate: only it compiles again, and it counts as rebuilt.
    let lib = workspace.join("beta").join("src").join("lib.rs");
    let text = fs::read_to_string(&lib).unwrap();
    fs::write(&lib, format!("{text}pub fn two() -> u32 {{ 2 }}\n")).unwrap();
    cargo(&workspace, &shims, &units, &["build"]);
    let second = read_units(&units);
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].crate_name, "beta");
    assert!(second[0].rebuilt);

    // Nothing changed: nothing compiles and nothing is recorded.
    cargo(&workspace, &shims, &units, &["build"]);
    assert!(read_units(&units).is_empty());

    // Tests: the test builds are recorded in test mode, and each test process by the runner.
    cargo(&workspace, &shims, &units, &["test", "--lib"]);
    let tests = read_units(&units);
    let mut built: Vec<_> = tests
        .iter()
        .filter(|u| u.mode == "test")
        .map(|u| u.crate_name.as_str())
        .collect();
    built.sort_unstable();
    assert_eq!(built, ["alpha", "beta"]);
    let mut ran: Vec<_> = tests
        .iter()
        .filter(|u| u.mode == RUN_MODE)
        .map(|u| u.crate_name.as_str())
        .collect();
    ran.sort_unstable();
    assert_eq!(ran, ["alpha", "beta"]);
    fs::remove_dir_all(&root).unwrap();
}
