//! A daemon restarted while a verb's command ran under a real supervisor: the verb run again
//! under the same handle takes the command over instead of starting it again (see
//! `ignite_daemon::adopt`).

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::ids::Handle;
use ignite_core::protocol::Class;
use ignite_core::time::SystemClock;
use ignite_core::traits::{CommandSpec, Launcher};
use ignite_daemon::adopt::{AdoptingLauncher, HANDLE_VARIABLE};
use ignite_daemon::steps::{Kind, Steps, Work};
use ignite_daemon::supervise::SupervisedLauncher;

fn scratch(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-adopt-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn launcher(root: &Path) -> SupervisedLauncher {
    SupervisedLauncher::new(Path::new(env!("CARGO_BIN_EXE_ignite")), root.join("runs"))
}

/// A command that waits until the test creates `go`, then prints and exits 3.
fn waiting(cwd: &Path, go: &Path) -> CommandSpec {
    CommandSpec {
        args: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "while [ ! -e '{}' ]; do sleep 0.01; done; echo finished; exit 3",
                go.display()
            ),
        ],
        cwd: cwd.to_path_buf(),
        env: Vec::new(),
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
    }
}

fn runs(root: &Path) -> usize {
    fs::read_dir(root.join("runs"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().join("spec.json").is_file())
        .count()
}

#[test]
fn a_verb_run_again_after_a_restart_takes_over_its_supervised_command() {
    let root = scratch("take-over");
    let tree = root.join("tree-01");
    fs::create_dir_all(&tree).unwrap();
    let go = root.join("go");
    let spec = waiting(&tree, &go);
    // The first daemon starts the command for handle T-4, then goes away.
    let first = launcher(&root);
    let env = vec![(HANDLE_VARIABLE.to_owned(), "T-4".to_owned())];
    drop(first.start(&spec, env, 0.0).unwrap());
    drop(first);

    let adopting = AdoptingLauncher::over_supervised(Arc::new(launcher(&root)), &[]);
    assert_eq!(adopting.waiting(), 1);
    let steps = Steps::new(Arc::new(SystemClock));
    let activity = steps.begin(Kind::Verb("test".to_owned()), Work::default());
    activity.attach_handle(&Handle::new("T-4").unwrap());
    let _entered = activity.enter();
    fs::write(&go, "").unwrap();
    let result = adopting.run(&spec);

    assert_eq!(result.exit, Some(3));
    assert_eq!(fs::read_to_string(&result.output).unwrap(), "finished\n");
    assert_eq!(runs(&root), 1, "nothing was started again");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_in_a_tree_reset_at_start_is_ended_and_not_taken_over() {
    let root = scratch("reset");
    let tree = root.join("landing-1");
    fs::create_dir_all(&tree).unwrap();
    let spec = waiting(&tree, &root.join("never"));
    let first = launcher(&root);
    let env = vec![(HANDLE_VARIABLE.to_owned(), "L-2".to_owned())];
    drop(first.start(&spec, env, 0.0).unwrap());
    drop(first);

    let second = launcher(&root);
    let left = second.unfinished();
    let adopting = AdoptingLauncher::over_supervised(Arc::new(second), &[tree]);
    assert_eq!(adopting.waiting(), 0);
    // Its supervisor writes a result with no exit code once the command has been ended.
    assert_eq!(left[0].wait().exit, None);
    fs::remove_dir_all(&root).unwrap();
}
