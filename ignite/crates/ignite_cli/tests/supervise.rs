//! Supervisors started from the real `ignite` executable: the result file they write, and a
//! command that outlives the daemon side that started it.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Launcher};
use ignite_daemon::supervise::{ResultFile, SupervisedLauncher};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-supervise-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn launcher(root: &Path) -> SupervisedLauncher {
    SupervisedLauncher::new(Path::new(env!("CARGO_BIN_EXE_ignite")), root.join("runs"))
}

fn shell(cwd: &Path, script: &str) -> CommandSpec {
    CommandSpec {
        args: vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
        cwd: cwd.to_path_buf(),
        env: vec![("GREETING".to_owned(), "hello".to_owned())],
        class: Class::Agent,
        cores: 3,
        resources: Vec::new(),
        tree: None,
        ticket: None,
    }
}

#[test]
fn a_supervised_command_leaves_a_result_file_with_its_exit_output_and_usage() {
    let root = scratch("result");
    let launcher = launcher(&root);
    // A child of the command, so the group's descendants are measured too.
    let spec = shell(
        &root,
        r#"echo "$GREETING from $(pwd) with $IGNITE_CORES cores"; echo oops >&2; /bin/sh -c 'i=0; while [ $i -lt 20000 ]; do i=$((i+1)); done'; exit 7"#,
    );
    let result = launcher.run(&spec);

    assert_eq!(result.exit, Some(7));
    let output = fs::read_to_string(&result.output).unwrap();
    let cwd = fs::canonicalize(&root).unwrap();
    assert!(
        output.contains(&format!("hello from {} with 3 cores", cwd.display())),
        "{output}"
    );
    assert!(output.contains("oops"), "{output}");
    assert!(result.usage.cpu_s > 0.0);
    assert!(result.usage.instructions.unwrap_or(0) > 0);

    // The same result is on disk, beside the output.
    let dir = result.output.parent().unwrap();
    let file: ResultFile =
        serde_json::from_slice(&fs::read(dir.join("result.json")).unwrap()).unwrap();
    assert_eq!(file.exit, Some(7));
    assert_eq!(file.output, result.output);
    assert_eq!(file.usage, result.usage);
    // Collected, so a restarted daemon does not pick it up again.
    assert!(launcher.unfinished().is_empty());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_outlives_the_daemon_side_and_its_result_is_picked_up_afterwards() {
    let root = scratch("restart");
    let go = root.join("go");
    // The command waits for the test to create `go`, so it is certainly still running when
    // the daemon side has gone.
    let spec = shell(
        &root,
        &format!(
            r"while [ ! -e '{}' ]; do sleep 0.01; done; echo finished",
            go.display()
        ),
    );
    let first = launcher(&root);
    let started = first.start(&spec, Vec::new(), 2.0).unwrap();
    let run_dir = started.run().dir().to_path_buf();
    // The daemon side ends: it stops waiting and its launcher is gone. Nothing ends the
    // supervisor or its command.
    drop(started);
    drop(first);

    // A new daemon finds the run, and its supervisor still holds the run's lock.
    let second = launcher(&root);
    let unfinished = second.unfinished();
    assert_eq!(unfinished.len(), 1);
    assert_eq!(unfinished[0].dir(), run_dir);
    let lock = fs::File::open(run_dir.join("lock")).unwrap();
    assert!(lock.try_lock_shared().is_err(), "the supervisor has exited");
    assert!(!run_dir.join("result.json").exists());

    fs::write(&go, "").unwrap();
    let result = unfinished[0].wait();
    assert_eq!(result.exit, Some(0));
    assert_eq!(fs::read_to_string(&result.output).unwrap(), "finished\n");
    assert!((result.usage.queued_s - 2.0).abs() < f64::EPSILON);
    assert!(second.unfinished().is_empty());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_that_cannot_start_reports_why_in_its_output() {
    let root = scratch("missing");
    let mut spec = shell(&root, "");
    spec.args = vec![root.join("no-such-program").display().to_string()];
    let result = launcher(&root).run(&spec);
    assert_eq!(result.exit, Some(127));
    let output = fs::read_to_string(&result.output).unwrap();
    assert!(output.contains("cannot run"), "{output}");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn ending_a_run_ends_its_command_and_every_process_it_started() {
    let root = scratch("end");
    // The command starts a child of its own; both wait forever.
    let spec = shell(&root, "/bin/sh -c 'while :; do sleep 1; done' & wait");
    let started = launcher(&root).start(&spec, Vec::new(), 0.0).unwrap();
    let run = started.run().clone();
    // `start` returns once the command has started and its group is known.
    assert!(run.dir().join("group").exists());
    run.end().unwrap();
    let result = started.wait();
    assert_eq!(result.exit, None);
    fs::remove_dir_all(&root).unwrap();
}
