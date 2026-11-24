//! What the wrapper reads from compiler arguments, and what a shim directory holds.
//! A real build through the wrapper is tested in `ignite_cli`'s `tests/integration/wrap.rs`.

use super::*;

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn scratch(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-wrap-{name}-{}", std::process::id()));
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

/// A shim directory holds the test runner and compiler wrapper and no `cargo`, and writing it deletes the `cargo`
/// shim an earlier release left there, so nothing in it can divert a plain `cargo` call.
#[test]
fn a_shim_directory_holds_no_cargo() {
    let dir = scratch("no-cargo");
    for retired in [".ignite-shims", "cargo", "cargo.cmd"] {
        fs::write(dir.join(retired), "old").unwrap();
    }
    let ignite = dir.join("ignite-exe");
    fs::write(&ignite, "program").unwrap();
    write_shims(&dir, &ignite).unwrap();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.retain(|name| name != "ignite-exe");
    let mut expected = vec![RUNNER_SCRIPT.to_owned(), RUSTC_WRAP_SCRIPT.to_owned()];
    expected.sort();
    assert_eq!(names, expected);
    let wrapper = fs::read_to_string(dir.join(RUSTC_WRAP_SCRIPT)).unwrap();
    // The POSIX shim names the executable and subcommand in its shell script.
    #[cfg(unix)]
    assert!(wrapper.contains("ignite-exe") && wrapper.contains("rustc-wrap"));
    #[cfg(windows)]
    assert_eq!(wrapper, "program");
    // Writing again over unchanged scripts succeeds and changes nothing.
    write_shims(&dir, &ignite).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join(RUSTC_WRAP_SCRIPT)).unwrap(),
        wrapper
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cargo_env_points_cargo_at_the_scripts() {
    let env = cargo_env(Path::new("/s"), Path::new("/u/units.jsonl"));
    let wrapper = Path::new("/s")
        .join(RUSTC_WRAP_SCRIPT)
        .display()
        .to_string();
    assert!(env.contains(&("RUSTC_WRAPPER".to_owned(), wrapper)));
    assert!(env.contains(&(UNITS_ENV.to_owned(), "/u/units.jsonl".to_owned())));
    let (runner, value) = &env[1];
    assert!(runner.starts_with("CARGO_TARGET_") && runner.ends_with("_RUNNER"));
    assert_eq!(runner, &runner.to_uppercase());
    assert_eq!(
        value,
        &Path::new("/s").join(RUNNER_SCRIPT).display().to_string()
    );
}

/// On Windows a shim is `ignite.exe` itself, linked or copied, not a batch file: `cmd.exe`
/// refuses a command line over 8191 characters, which a `windows-sys` compile exceeds.
#[cfg(windows)]
#[test]
fn a_shim_is_the_ignite_executable_itself() {
    let dir = scratch("exe-shims");
    let ignite = dir.join("ignite.exe");
    fs::write(&ignite, "program").unwrap();
    let shims = dir.join("shims");
    write_shims(&shims, &ignite).unwrap();
    for name in [RUSTC_WRAP_SCRIPT, RUNNER_SCRIPT] {
        assert_eq!(fs::read_to_string(shims.join(name)).unwrap(), "program");
    }
    // A batch shim from an earlier release is removed, and a second write changes nothing.
    fs::write(shims.join("rustc-wrap.cmd"), "old").unwrap();
    write_shims(&shims, &ignite).unwrap();
    assert!(!shims.join("rustc-wrap.cmd").exists());
    fs::remove_dir_all(&dir).unwrap();
}
