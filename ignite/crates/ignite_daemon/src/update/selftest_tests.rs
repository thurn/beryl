use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::fakes::FakeLauncher;
use ignite_core::traits::RunResult;

use super::*;

fn tempdir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ignite-selftest-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A repository with a small `ignite.toml` and one check manifest, and the machine it declares.
/// The real `ignite.toml` and manifests are parsed by `ignite_core`'s tests; reading them here
/// would make every test of this crate rerun whenever a check manifest changes.
fn repository() -> (PathBuf, String) {
    let repo = tempdir();
    std::fs::write(
        repo.join("ignite.toml"),
        "[repository]\nremote = \"origin\"\nstaging = \"staging\"\nmaster = \"master\"\n\
         trees = \".trees\"\nchecks = \"manifests\"\n\n[hub]\nurl = \"https://hub\"\n\
         project = \"p\"\n\n[gate]\nthreshold = 0.1\ncap = 10\n\n[machine.m]\nticket_trees = 1\n\
         landing_trees = 1\nfloor = { batch = 1, tip = 1 }\n",
    )
    .unwrap();
    let checks = repo.join("manifests");
    std::fs::create_dir_all(&checks).unwrap();
    std::fs::write(
        checks.join("py.toml"),
        "[family.py]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
         list = \"l\"\nrun = \"r\"\ninputs = [\"src/**\"]\n",
    )
    .unwrap();
    (repo, "m".to_owned())
}

fn home_with_database() -> Home {
    let home = Home { root: tempdir() };
    let conn = ignite_store::local::open(&home.root.join("daemon.db")).unwrap();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('selftest-marker', 'kept')",
        [],
    )
    .unwrap();
    home
}

#[test]
fn a_repository_that_parses_and_a_database_that_migrates_pass() {
    let (repo, machine) = repository();
    let home = home_with_database();
    check(&repo, &home, &machine).unwrap();
}

#[test]
fn a_machine_with_no_database_yet_passes() {
    let (repo, machine) = repository();
    let home = Home { root: tempdir() };
    check(&repo, &home, &machine).unwrap();
    assert!(!home.root.join("daemon.db").exists());
}

#[test]
fn a_manifest_that_does_not_parse_fails() {
    let (repo, machine) = repository();
    std::fs::write(repo.join("manifests/broken.toml"), "[family.x\nnot toml").unwrap();
    let error = check(&repo, &home_with_database(), &machine).unwrap_err();
    assert!(error.contains("broken.toml"), "{error}");
}

#[test]
fn an_ignite_toml_that_does_not_parse_fails() {
    let (repo, machine) = repository();
    std::fs::write(repo.join("ignite.toml"), "[repository\n").unwrap();
    assert!(check(&repo, &home_with_database(), &machine).is_err());
}

#[test]
fn a_machine_the_repository_does_not_declare_fails() {
    let (repo, _) = repository();
    let error = check(&repo, &home_with_database(), "no-such-machine").unwrap_err();
    assert!(error.contains("no-such-machine"), "{error}");
}

#[test]
fn a_database_newer_than_the_build_fails_and_is_not_changed() {
    let (repo, machine) = repository();
    let home = home_with_database();
    let live = home.root.join("daemon.db");
    {
        let conn = rusqlite::Connection::open(&live).unwrap();
        conn.execute(
            "UPDATE meta SET value = '999999' \
             WHERE key IN ('schema_version', 'min_reader_version')",
            [],
        )
        .unwrap();
    }
    let before = std::fs::read(&live).unwrap();
    let error = check(&repo, &home, &machine).unwrap_err();
    assert!(error.contains("migrations fail"), "{error}");
    assert_eq!(std::fs::read(&live).unwrap(), before);
}

#[test]
fn the_migrations_run_on_a_copy_and_leave_the_live_database_alone() {
    let home = Home { root: tempdir() };
    let live = home.root.join("daemon.db");
    // An old database: only the first migration applied.
    {
        let mut conn = rusqlite::Connection::open(&live).unwrap();
        ignite_store::migrate::apply(&mut conn, &ignite_store::local::MIGRATIONS[..1]).unwrap();
    }
    let before = std::fs::read(&live).unwrap();
    migrate_a_copy(&live).unwrap();
    assert_eq!(std::fs::read(&live).unwrap(), before);
    let conn = rusqlite::Connection::open(&live).unwrap();
    assert_eq!(ignite_store::migrate::version(&conn).unwrap(), 1);
}

fn finished(exit: Option<i32>, hung: bool, output: &str) -> RunResult {
    let dir = tempdir();
    let path = dir.join("output.log");
    std::fs::write(&path, output).unwrap();
    RunResult {
        exit,
        output: path,
        usage: ignite_core::protocol::Usage::default(),
        hung,
    }
}

fn supervised(launcher: &Arc<FakeLauncher>) -> SupervisedSelftest {
    SupervisedSelftest {
        launcher: launcher.clone(),
        repo: PathBuf::from("/repo"),
    }
}

#[test]
fn the_selftest_runs_the_new_executable_in_the_checkout() {
    let launcher = Arc::new(FakeLauncher::default());
    launcher.script(&["/bin/new"], finished(Some(0), false, "selftest passed\n"));
    let output = supervised(&launcher).run(Path::new("/bin/new")).unwrap();
    assert_eq!(output, "selftest passed\n");
    let runs = launcher.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].args, ["/bin/new", "internal", "selftest"]);
    assert_eq!(runs[0].cwd, PathBuf::from("/repo"));
}

#[test]
fn a_failing_or_hung_selftest_is_an_error_with_its_output() {
    let launcher = Arc::new(FakeLauncher::default());
    launcher.script(
        &["/bin/bad"],
        finished(Some(1), false, "ignite selftest: no manifest\n"),
    );
    let error = supervised(&launcher)
        .run(Path::new("/bin/bad"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("exit 1") && error.contains("no manifest"),
        "{error}"
    );

    launcher.script(&["/bin/hung"], finished(None, true, "waiting\n"));
    let error = supervised(&launcher)
        .run(Path::new("/bin/hung"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("was ended"), "{error}");
}
