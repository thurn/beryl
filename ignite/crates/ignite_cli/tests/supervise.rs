//! Supervisors started from the real `ignite` executable: the result file they write, a command
//! that outlives the daemon side that started it, and grants that grow and shrink while their
//! command runs.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;

use ignite_core::config::Floor;
use ignite_core::fakes::FakeClock;
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Launcher};
use ignite_daemon::sched::{GrantRequest, Limits, Scheduler};
use ignite_daemon::supervise::{ResultFile, SupervisedLauncher};
use ignite_os::hostcpu::HostCpu;

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
        grows: false,
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

/// A host that reads as idle, so the scheduler grants every core.
struct IdleHost;

impl HostCpu for IdleHost {
    fn cores(&self) -> u32 {
        6
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(0.0)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Err(io::Error::other("not read"))
    }
}

/// A scheduler for six cores, so one command takes at most four, whose clock never moves: a
/// request that waits for shrinking grants waits until they have shrunk.
fn six_cores() -> Arc<Scheduler> {
    let limits = Limits {
        cores: 6,
        floor: Floor::default(),
        resources: BTreeMap::new(),
    };
    Arc::new(Scheduler::new(
        limits,
        Arc::new(FakeClock::new(Timestamp::EPOCH)),
        Arc::new(IdleHost),
    ))
}

fn ask(class: Class, cores: u32) -> GrantRequest {
    GrantRequest {
        class,
        cores,
        resources: Vec::new(),
        memory_mb: None,
        shrinks: false,
        grows: false,
        cores_file: None,
    }
}

/// A FIFO the command writes a line to when it reaches a point, and the test reads, which
/// blocks until then.
fn signal(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    let made = Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(made.success());
    path
}

/// A command started with a growing grant reads `IGNITE_CORES_FILE` and sees it rise when
/// another grant is released.
#[test]
fn a_growing_command_sees_its_cores_file_rise_when_cores_free_up() {
    let root = scratch("grows");
    let scheduler = six_cores();
    let holder = scheduler.acquire(&ask(Class::Agent, 4));
    let started = signal(&root, "started");
    let mut spec = shell(
        &root,
        r#"echo "first $(cat "$IGNITE_CORES_FILE")"; echo go > "$STARTED"
        i=0; while [ "$(cat "$IGNITE_CORES_FILE")" != 4 ] && [ $i -lt 3000 ]; do sleep 0.01; i=$((i+1)); done
        echo "last $(cat "$IGNITE_CORES_FILE")""#,
    );
    spec.cores = 4;
    spec.grows = true;
    spec.env
        .push(("STARTED".to_owned(), started.display().to_string()));
    let launcher = launcher(&root).with_scheduler(Arc::clone(&scheduler));
    let run = thread::spawn(move || launcher.run(&spec));

    fs::read_to_string(&started).unwrap();
    scheduler.release(&holder);
    let result = run.join().unwrap();

    assert_eq!(result.exit, Some(0));
    let output = fs::read_to_string(&result.output).unwrap();
    assert!(output.contains("first 2\n"), "{output}");
    assert!(output.contains("last 4\n"), "{output}");
    fs::remove_dir_all(&root).unwrap();
}

/// A Cargo build that only compiles runs under the supervisor's jobserver, without
/// `CARGO_BUILD_JOBS`. Its job slots grow when another grant is released, and it gives cores
/// back while a landing waits, so the landing gets them before the build ends.
#[test]
fn a_cargo_build_takes_job_slots_as_its_grant_grows_and_shrinks_for_a_landing() {
    let root = scratch("cargo");
    let scheduler = six_cores();
    let holder = scheduler.acquire(&ask(Class::Agent, 4));
    let started = signal(&root, "started");
    let grew = signal(&root, "grew");
    let go = signal(&root, "go");
    // A stand-in for Cargo that takes slots from the jobserver as a compiler job would: each
    // read of a byte blocks until the FIFO holds one.
    let cargo = root.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
[ -z "$CARGO_BUILD_JOBS" ] || { echo "CARGO_BUILD_JOBS is $CARGO_BUILD_JOBS"; exit 1; }
exec 3<>"${CARGO_MAKEFLAGS#*fifo:}"
dd bs=1 count=1 <&3 >/dev/null 2>&1
echo go > "$STARTED"
dd bs=1 count=2 <&3 >/dev/null 2>&1
echo "took 3 slots"
printf '+++' >&3
echo go > "$GREW"
read _ < "$GO"
echo "cores $(cat "$IGNITE_CORES_FILE")"
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut spec = shell(&root, "");
    spec.args = vec![cargo.display().to_string(), "build".to_owned()];
    spec.cores = 4;
    spec.env.extend([
        ("STARTED".to_owned(), started.display().to_string()),
        ("GREW".to_owned(), grew.display().to_string()),
        ("GO".to_owned(), go.display().to_string()),
        ("CARGO_BUILD_JOBS".to_owned(), "9".to_owned()),
    ]);
    let launcher = launcher(&root).with_scheduler(Arc::clone(&scheduler));
    let run = thread::spawn(move || launcher.run(&spec));

    // Granted two cores: one slot of its own and one byte in the FIFO.
    fs::read_to_string(&started).unwrap();
    scheduler.release(&holder);
    // Grown to four: the supervisor wrote two more bytes, which the build took.
    fs::read_to_string(&grew).unwrap();
    // The landing waits for the build's cores and gets them one per scheduler step.
    let land = scheduler.acquire(&ask(Class::Land, 4));
    assert_eq!(land.cores, 4);
    fs::write(&go, "go\n").unwrap();
    let result = run.join().unwrap();
    scheduler.release(&land);

    let output = fs::read_to_string(&result.output).unwrap();
    assert_eq!(result.exit, Some(0), "{output}");
    assert!(output.contains("took 3 slots\n"), "{output}");
    assert!(output.contains("cores 2\n"), "{output}");
    fs::remove_dir_all(&root).unwrap();
}
