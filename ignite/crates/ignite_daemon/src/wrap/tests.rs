//! What the wrapper reads from compiler arguments, and the `cargo` shim's choice of `cargo`.
//! A real build through the wrapper is tested in `ignite_cli`'s `tests/wrap.rs`.

use std::process::Output;

use super::*;

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-wrap-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cargo_probes_are_not_compile_units() {
    assert_eq!(CompileArgs::parse(&strings(&["rustc", "-vV"])), None);
    assert_eq!(
        CompileArgs::parse(&strings(&[
            "rustc",
            "-",
            "--crate-name",
            "___",
            "--print=file-names"
        ])),
        None
    );
}

#[test]
fn the_mode_comes_from_the_driver_and_the_flags() {
    let mode = |args: &[&str]| CompileArgs::parse(&strings(args)).unwrap().mode;
    assert_eq!(
        mode(&["rustc", "--crate-name", "a", "--emit=dep-info,metadata"]),
        "check"
    );
    assert_eq!(
        mode(&[
            "rustc",
            "--crate-name",
            "a",
            "--emit=dep-info,metadata,link"
        ]),
        "build"
    );
    assert_eq!(mode(&["rustc", "--crate-name", "a", "--test"]), "test");
    assert_eq!(
        mode(&[
            "/x/clippy-driver",
            "rustc",
            "--crate-name",
            "a",
            "--emit=dep-info,metadata"
        ]),
        "clippy"
    );
}

#[test]
fn a_unit_is_rebuilt_when_its_output_directory_already_holds_its_files() {
    let dir = scratch("rebuilt");
    let args = strings(&[
        "rustc",
        "--crate-name",
        "alpha",
        "--out-dir",
        dir.to_str().unwrap(),
        "-C",
        "extra-filename=-0123abcd",
    ]);
    let unit = CompileArgs::parse(&args).unwrap();
    assert_eq!(unit.extra_filename, "-0123abcd");
    assert!(!unit.built_before());
    // Another unit's files, and a crate whose name only starts the same, do not count.
    fs::write(dir.join("libalpha-ffff0000.rlib"), "").unwrap();
    fs::write(dir.join("libalpha_beta-0123abcd.rlib"), "").unwrap();
    assert!(!unit.built_before());
    fs::write(dir.join("alpha-0123abcd.d"), "").unwrap();
    assert!(unit.built_before());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_test_binary_is_named_by_its_crate() {
    assert_eq!(
        test_binary_crate(Path::new("/t/deps/ignite_core-0f1e2d3c4b5a6978")),
        "ignite_core"
    );
    assert_eq!(test_binary_crate(Path::new("/t/deps/plain")), "plain");
}

/// Runs the `cargo` shim in `shims` with `PATH` set to `path` and the given extra variables.
fn run_shim(shims: &Path, path: &[&Path], env: &[(&str, &str)]) -> Output {
    let path = std::env::join_paths(path).unwrap();
    let mut command = Command::new(shims.join("cargo"));
    command
        .args(["build", "--release"])
        .env_clear()
        .env("PATH", path);
    for (k, v) in env {
        command.env(k, v);
    }
    command.output().unwrap()
}

fn script(path: &Path, text: &str) {
    fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_cargo_shim_sends_cargo_through_ignite_run_unless_a_grant_is_held() {
    let root = scratch("shim");
    let real = root.join("toolchain");
    fs::create_dir_all(&real).unwrap();
    script(&real.join("cargo"), r#"echo "real cargo $*""#);
    // Every shell utility the shims use is a shell builtin, except what PATH supplies.
    let ignite = root.join("bin with space").join("ignite");
    fs::create_dir_all(ignite.parent().unwrap()).unwrap();
    script(&ignite, r#"echo "ignite $*""#);
    let ours = root.join("tree-01-shims");
    let other = root.join("tree-02-shims");
    write_shims(&ours, &ignite).unwrap();
    write_shims(&other, &ignite).unwrap();

    // The other tree's shim directory comes first on PATH and is skipped too.
    let output = run_shim(&ours, &[&other, &ours, &real], &[]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("ignite run -- {}/cargo build --release", real.display())
    );

    let inside = run_shim(&ours, &[&ours, &real], &[("IGNITE_CORES", "4")]);
    assert_eq!(
        String::from_utf8(inside.stdout).unwrap().trim(),
        "real cargo build --release"
    );

    let none = run_shim(&ours, &[&ours], &[]);
    assert_eq!(none.status.code(), Some(127));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn cargo_env_points_cargo_at_the_scripts() {
    let env = cargo_env(Path::new("/s"), Path::new("/u/units.jsonl"));
    assert!(env.contains(&("RUSTC_WRAPPER".to_owned(), "/s/rustc-wrap".to_owned())));
    assert!(env.contains(&(UNITS_ENV.to_owned(), "/u/units.jsonl".to_owned())));
    let (runner, value) = &env[1];
    assert!(runner.starts_with("CARGO_TARGET_") && runner.ends_with("_RUNNER"));
    assert_eq!(runner, &runner.to_uppercase());
    assert_eq!(value, "/s/test-runner");
}
