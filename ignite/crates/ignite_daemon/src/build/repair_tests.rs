//! Reading a failed build's output and clearing a crate from a target directory.

use super::*;

#[test]
fn a_failed_build_names_the_crates_of_its_targets() {
    let output = "\
error: linking with `cc` failed: exit status: 1
error: could not compile `game_session` (test \"session\") due to 1 previous error
error: could not compile `game-app` (lib) due to 2 previous errors
error: could not compile `game_cli` (bin \"game-cli\" test) due to 1 previous error
error: could not compile `game_session` (test \"session\") due to 1 previous error
error: could not compile `zstd-sys` (build script) due to 1 previous error
error: could not compile `old`
";
    assert_eq!(
        compile_failure(output),
        Some(CompileFailure {
            crates: vec![
                "session".to_owned(),
                "game_app".to_owned(),
                "game_cli".to_owned(),
                "old".to_owned(),
            ],
            damaged: true,
        })
    );
}

#[test]
fn a_source_error_is_not_damage_and_an_output_without_crates_is_no_compile_failure() {
    let source = "error[E0308]: mismatched types\n\
                  error: could not compile `game_rules` (lib) due to 1 previous error\n";
    assert_eq!(
        compile_failure(source),
        Some(CompileFailure {
            crates: vec!["game_rules".to_owned()],
            damaged: false,
        })
    );
    assert_eq!(compile_failure("error: failed to load manifest\n"), None);
}

/// A link error names the library that holds the bad reference, which is cleared too; the
/// libraries on the linker's command line are not.
#[test]
fn a_link_failure_names_the_libraries_the_linker_reports() {
    let macos = "\
error: linking with `cc` failed: exit status: 1
  = note:  \"cc\" \"/t/debug/deps/{libgame_inspect-0556e3158244f6c4,libserde-1f1a41c00f3c1847}.rlib\" \"-o\" \"/t/debug/deps/record-cda05fcc31e2b356\"
          Undefined symbols for architecture arm64:
            \"_anon.2e5a857eea634338498684503b2aa78f.5.llvm.12721422738467408669\", referenced from:
                __RINvXs4_10serde_json in libgame_inspect-0556e3158244f6c4.rlib[43](game_inspect-0556e3158244f6c4.41y.04s541b.rcgu.o)
                __RINvXs5_10serde_json in libgame_inspect-0556e3158244f6c4.rlib[80](game_inspect-0556e3158244f6c4.9fo.04s541b.rcgu.o)
          ld: symbol(s) not found for architecture arm64

error: could not compile `game_record` (bin \"record\") due to 1 previous error
";
    assert_eq!(
        compile_failure(macos).map(|f| f.crates),
        Some(vec!["record".to_owned(), "game_inspect".to_owned()])
    );
    let linux = "\
error: linking with `cc` failed: exit status: 1
  = note: /usr/bin/ld: /t/debug/deps/libgame_scene-1c904d4dab1879b1.rlib(game_scene-1c904d4dab1879b1.a1.rcgu.o): in function `draw':
          undefined reference to `_anon.e15180dca39ec90f.2.llvm.168'
error: could not compile `game_record` (bin \"record\") due to 1 previous error
";
    assert_eq!(
        compile_failure(linux).map(|f| f.crates),
        Some(vec!["record".to_owned(), "game_scene".to_owned()])
    );
    // A source error leaves libraries alone, even one the output happens to name.
    let source = "error[E0308]: mismatched types in libgame_scene-1c904d4dab1879b1.rlib(x.o)\n\
                  error: could not compile `game_rules` (lib) due to 1 previous error\n";
    assert_eq!(
        compile_failure(source).map(|f| f.crates),
        Some(vec!["game_rules".to_owned()])
    );
}

#[test]
fn the_units_of_a_crate_are_its_hashed_deps_files() {
    let dir = scratch("units");
    let profile = dir.join("debug");
    for name in [
        "session-1d2e56b5d8df6699",
        "session-1d2e56b5d8df6699.d",
        "libsession-2d2e56b5d8df6699.rlib",
        "session-3d2e56b5d8df6699.0syg23f.06txj64.rcgu.o",
        "session.02hjnukd9tark93exg4ca0ha2.0syg23f.rcgu.o",
        "session_types-4d2e56b5d8df6699",
        "session-notahash",
        "libgame_session-5d2e56b5d8df6699.rlib",
    ] {
        touch(&profile.join("deps").join(name));
    }
    assert_eq!(
        units_of(&profile, "session"),
        [
            "session-1d2e56b5d8df6699",
            "session-2d2e56b5d8df6699",
            "session-3d2e56b5d8df6699"
        ]
    );
    assert!(is_loose_object(
        "session.02hjnukd9tark93exg4ca0ha2.0syg23f.rcgu.o",
        "session"
    ));
    assert!(!is_loose_object(
        "session-3d2e56b5d8df6699.0syg23f.06txj64.rcgu.o",
        "session"
    ));
    let _ = fs::remove_dir_all(&dir);
}

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-repair-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// Writes an empty file, creating its folders.
fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

/// A profile directory with two units of crate `proto`: the library `proto-aaaaaaaaaaaaaaaa`,
/// whose last compile was ended partway, and the test build `proto-bbbbbbbbbbbbbbbb`, which
/// finished. Each has a fingerprint folder, `deps` files, codegen unit objects and an
/// incremental folder holding the same codegen units.
fn two_units(dir: &Path) -> PathBuf {
    let profile = dir.join("debug");
    for (unit, cgu, incremental, session) in [
        (
            "proto-aaaaaaaaaaaaaaaa",
            "0libcgu",
            "proto-0lib",
            "s-hoam-1-working",
        ),
        (
            "proto-bbbbbbbbbbbbbbbb",
            "0testcgu",
            "proto-1test",
            "s-hoam-2-abcdef",
        ),
    ] {
        touch(&profile.join(".fingerprint").join(unit).join("lib-proto"));
        touch(&profile.join("deps").join(format!("{unit}.d")));
        touch(&profile.join("deps").join(format!("lib{unit}.rlib")));
        touch(
            &profile
                .join("deps")
                .join(format!("{unit}.{cgu}.06txj64.rcgu.o")),
        );
        touch(
            &profile
                .join("incremental")
                .join(incremental)
                .join(session)
                .join(format!("{cgu}.o")),
        );
    }
    touch(&dir.join(SUSPECT_FILE));
    profile
}

#[test]
fn a_build_ended_partway_clears_only_the_unit_it_left_unfinished() {
    let dir = scratch("one-unit");
    let profile = two_units(&dir);
    assert_eq!(clear_if_suspect(&dir), ["proto-aaaaaaaaaaaaaaaa"]);
    let deps = names(&profile.join("deps"));
    assert!(deps.iter().all(|n| !n.contains("aaaa")), "{deps:?}");
    assert_eq!(deps.len(), 3, "the test build keeps its files: {deps:?}");
    assert_eq!(
        names(&profile.join(".fingerprint")),
        ["proto-bbbbbbbbbbbbbbbb"]
    );
    assert_eq!(names(&profile.join("incremental")), ["proto-1test"]);
    assert!(!dir.join(SUSPECT_FILE).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_unfinished_first_compile_loses_only_its_incremental_folder() {
    let dir = scratch("first");
    let profile = two_units(&dir);
    touch(
        &profile
            .join("incremental")
            .join("proto-2new")
            .join("s-hoam-3-working")
            .join("dep-graph.part.bin"),
    );
    fs::remove_dir_all(profile.join("incremental").join("proto-0lib")).unwrap();
    assert_eq!(clear_if_suspect(&dir), ["proto-2new"]);
    assert_eq!(names(&profile.join("deps")).len(), 6);
    assert_eq!(names(&profile.join(".fingerprint")).len(), 2);
    assert_eq!(names(&profile.join("incremental")), ["proto-1test"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn clearing_a_crate_deletes_its_fingerprints_with_its_files() {
    let dir = scratch("crate");
    let profile = two_units(&dir);
    clear_crates(&dir, &["proto".to_owned()]);
    for folder in [".fingerprint", "deps", "incremental"] {
        assert!(names(&profile.join(folder)).is_empty(), "{folder}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_unit_whose_fingerprint_cannot_be_deleted_keeps_its_files() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("stuck");
    let profile = two_units(&dir);
    let fingerprints = profile.join(".fingerprint");
    fs::set_permissions(&fingerprints, fs::Permissions::from_mode(0o555)).unwrap();
    clear_crates(&dir, &["proto".to_owned()]);
    fs::set_permissions(&fingerprints, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(names(&fingerprints).len(), 2);
    assert_eq!(names(&profile.join("deps")).len(), 6);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_cross_target_profile_is_cleared_too() {
    let dir = std::env::temp_dir().join(format!("ignite-repair-{}-cross", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let profile = dir.join("x86_64-pc-windows-msvc").join("debug");
    fs::create_dir_all(profile.join("incremental").join("core-abc")).unwrap();
    fs::create_dir_all(profile.join("deps")).unwrap();
    fs::write(profile.join("deps").join("core-0123456789abcdef.exe"), "x").unwrap();
    let deleted = clear_crates(&dir, &["core".to_owned()]);
    assert_eq!(deleted.len(), 2, "{deleted:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_ended_command_marks_the_target_directories_it_may_have_compiled_in() {
    let dir = std::env::temp_dir().join(format!("ignite-repair-{}-ended", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for target in ["out", "rust/target", "other/target"] {
        fs::create_dir_all(dir.join(target)).unwrap();
    }
    let key = r#"{"tree":"tree-01","product":"p","packages":[],"hash":"h","outputs":["out"]}"#;
    let spec = |args: &[&str], cwd: &str, env: Vec<(String, String)>| SpecFile {
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: dir.join(cwd),
        env,
        queued_s: 0.0,
        jobserver: None,
        build_jobs: None,
        hang_limit_s: None,
        grant: None,
    };
    let build = spec(
        &["make"],
        "",
        vec![(BUILD_KEY_ENV.to_owned(), key.to_owned())],
    );
    mark_ended(&build, &dir);
    assert!(dir.join("out").join(SUSPECT_FILE).is_file());
    mark_ended(
        &spec(&["cargo", "nextest", "run"], "rust", Vec::new()),
        &dir,
    );
    assert!(dir.join("rust/target").join(SUSPECT_FILE).is_file());
    mark_ended(&spec(&["uv", "run", "pytest"], "other", Vec::new()), &dir);
    assert!(!dir.join("other/target").join(SUSPECT_FILE).exists());
    // A mark without unfinished compiles clears nothing and is removed.
    assert!(clear_if_suspect(&dir.join("out")).is_empty());
    assert!(!dir.join("out").join(SUSPECT_FILE).exists());
    let _ = fs::remove_dir_all(&dir);
}
