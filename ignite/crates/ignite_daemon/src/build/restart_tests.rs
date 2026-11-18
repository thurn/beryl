//! A restarted daemon's build manager: a second manager over the same database and runs folder.

use std::fs::File;

use ignite_store::local::builds;

use super::*;
use crate::supervise::{ResultFile, SpecFile, SupervisedLauncher};

fn database(f: &Fixture) -> PathBuf {
    f.dir.join("daemon.db")
}

/// `f`'s manager, keeping its builds in the database at `path`.
fn stored(f: Fixture, path: &Path) -> Fixture {
    let manager = Arc::into_inner(f.manager).expect("the fixture holds the only manager");
    let conn = ignite_store::local::open(path).unwrap();
    Fixture {
        manager: Arc::new(manager.with_store(conn)),
        ..f
    }
}

/// A new manager over the same tree and database, as after a daemon restart.
fn restarted(f: &Fixture, path: &Path) -> Fixture {
    stored(manager_in(f.dir.clone(), Arc::clone(&f.git), false), path)
}

fn rows(path: &Path) -> Vec<ignite_store::local::rows::Build> {
    builds::all(&ignite_store::local::open(path).unwrap()).unwrap()
}

#[test]
fn a_restarted_manager_starts_no_build_for_a_product_the_tree_holds() {
    let f = fixture("restart-held", false);
    let db = database(&f);
    let f = stored(f, &db);
    let first = f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(first, BuildResult::Built { .. }), "{first:?}");
    fs::create_dir_all(f.dir.join("target")).unwrap();
    fs::write(f.dir.join("target").join("CACHEDIR.TAG"), "x").unwrap();

    let again = restarted(&f, &db);
    assert_eq!(
        again.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
    assert!(again.launcher.builds().is_empty());
    // A changed input still builds.
    again.git.set_object("a", oid('2'));
    let changed = again.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(changed, BuildResult::Built { .. }), "{changed:?}");
}

/// Output directories deleted while no daemon ran mean the tree no longer holds the product.
#[test]
fn a_restarted_manager_builds_again_when_the_outputs_are_gone() {
    let f = fixture("restart-gone", false);
    let db = database(&f);
    let f = stored(f, &db);
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert_eq!(rows(&db).len(), 1);

    let again = restarted(&f, &db);
    assert!(rows(&db).is_empty(), "the stale row is dropped");
    let built = again.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert!(matches!(built, BuildResult::Built { .. }), "{built:?}");
    assert_eq!(again.launcher.builds().len(), 1);
}

#[test]
fn a_restarted_manager_answers_a_failed_build_with_its_failure() {
    let f = fixture("restart-failed", false);
    let db = database(&f);
    let f = stored(f, &db);
    f.launcher.with(|s| s.failing = Some((Some(1), false)));
    let BuildResult::Failed { output, .. } = f.manager.ensure(&tree(), "tests", &names(&["b"]))
    else {
        panic!("the build should fail");
    };

    let again = restarted(&f, &db);
    let answer = again.manager.ensure(&tree(), "tests", &names(&["b"]));
    assert_eq!(
        answer,
        BuildResult::Failed {
            output,
            usage: Usage::default(),
        }
    );
    assert!(again.launcher.builds().is_empty());
}

/// Forgetting a tree's builds also forgets them for the next daemon.
#[test]
fn forgetting_a_tree_clears_its_recorded_builds() {
    let f = fixture("restart-forget", false);
    let db = database(&f);
    let f = stored(f, &db);
    f.manager.ensure(&tree(), "tests", &names(&["b"]));
    f.manager.forget(&tree());
    assert!(rows(&db).is_empty());
}

/// A build command that a previous daemon started and that still runs is waited for, not run
/// again, and its result is stored once.
#[test]
fn a_build_running_across_a_restart_is_joined_and_stored_once() {
    // The spec of a build command as the previous daemon started it.
    let first = fixture("restart-join", false);
    first.manager.ensure(&tree(), "tests", &names(&["b"]));
    let started = first
        .launcher
        .with(|s| s.runs.last().cloned())
        .expect("a build ran");
    assert!(started.env.iter().any(|(name, _)| name == BUILD_KEY_ENV));

    // Its run directory, whose supervisor (the lock's holder) is still running.
    let runs = first.dir.join("runs");
    let run = runs.join("00000000000000000001-1-0");
    fs::create_dir_all(&run).unwrap();
    let spec = SpecFile {
        args: started.args.clone(),
        cwd: started.cwd.clone(),
        env: started.env.clone(),
        queued_s: 0.0,
        jobserver: None,
        build_jobs: None,
        hang_limit_s: None,
        grant: None,
    };
    fs::write(run.join("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
    let supervisor = File::create(run.join("lock")).unwrap();
    supervisor.lock().unwrap();

    let db = first.dir.join("restarted.db");
    let again = restarted(&first, &db);
    let unfinished = || SupervisedLauncher::new(Path::new("ignite"), runs.clone()).unfinished();
    // A run in a tree the restarted daemon resets is not joined.
    assert_eq!(
        again
            .manager
            .rejoin(unfinished(), std::slice::from_ref(&first.dir)),
        0
    );
    assert_eq!(again.manager.rejoin(unfinished(), &[]), 1);

    let request = {
        let manager = Arc::clone(&again.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &names(&["b"])))
    };
    until_waiting(&again.manager, (1, 0));
    let result = ResultFile {
        exit: Some(0),
        output: run.join("output.log"),
        usage: Usage::default(),
        hung: false,
    };
    fs::write(
        run.join("result.json"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    drop(supervisor);

    let joined = request.join().unwrap();
    assert!(matches!(joined, BuildResult::Built { .. }), "{joined:?}");
    assert!(again.launcher.builds().is_empty(), "nothing ran again");
    assert!(run.join("collected").exists());
    let stored = rows(&db);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].outcome, builds::HELD);
    assert_eq!(
        again.manager.ensure(&tree(), "tests", &names(&["b"])),
        BuildResult::Current
    );
    assert!(unfinished().is_empty());
}
