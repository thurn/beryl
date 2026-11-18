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

#[test]
fn outputs_of_a_crate_are_its_hashed_files_and_unit_objects() {
    assert!(is_output_of("session-1d2e56b5d8df6699", "session"));
    assert!(is_output_of("session-1d2e56b5d8df6699.d", "session"));
    assert!(is_output_of("libsession-1d2e56b5d8df6699.rlib", "session"));
    assert!(is_output_of(
        "session.02hjnukd9tark93exg4ca0ha2.0syg23f.rcgu.o",
        "session"
    ));
    assert!(!is_output_of("session_types-1d2e56b5d8df6699", "session"));
    assert!(!is_output_of("session-notahash", "session"));
    assert!(!is_output_of(
        "libgame_session-1d2e56b5d8df6699.rlib",
        "session"
    ));
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
