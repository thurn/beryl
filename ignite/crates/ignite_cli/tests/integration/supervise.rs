//! Supervisors started from the real `ignite` executable: the result file they write, a command
//! that outlives the daemon side that started it, and grants that grow and shrink while their
//! command runs.
//!
use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Launcher};
use ignite_daemon::supervise::{ResultFile, SupervisedLauncher};

use super::fixture;

fn scratch(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-supervise-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn launcher(root: &Path) -> SupervisedLauncher {
    SupervisedLauncher::new(Path::new(env!("CARGO_BIN_EXE_ignite")), root.join("runs"))
}

fn shell(cwd: &Path, script: &str) -> CommandSpec {
    CommandSpec {
        args: if cfg!(windows) {
            vec![
                "cmd".to_owned(),
                "/d".to_owned(),
                "/c".to_owned(),
                script.to_owned(),
            ]
        } else {
            vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
        },
        cwd: cwd.to_path_buf(),
        env: vec![("GREETING".to_owned(), "hello".to_owned())],
        class: Class::Agent,
        cores: 3,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    }
}

#[test]
fn a_supervised_command_leaves_a_result_file_with_its_exit_output_and_usage() {
    let root = scratch("result");
    let launcher = launcher(&root);
    let mut spec = fixture::spec(&root, "report");
    spec.cores = 3;
    spec.env.push(("GREETING".to_owned(), "hello".to_owned()));
    let result = launcher.run(&spec);

    assert_eq!(result.exit, Some(7));
    let output = fs::read_to_string(&result.output).unwrap();
    let cwd = ignite_os::paths::canonicalize(&root).unwrap();
    assert!(
        output.contains(&format!("hello from {} with 3 cores", cwd.display())),
        "{output}"
    );
    assert!(output.contains("oops"), "{output}");
    assert!(result.usage.cpu_s > 0.0);
    #[cfg(unix)]
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
fn a_command_is_ended_when_the_caller_that_asked_for_it_goes_away() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = scratch("cancel");
    let started = root.join("started");
    let spec = fixture::waiting(&root, &root.join("never"), 0);
    let launcher = launcher(&root);
    let gone = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            fixture::until(|| started.exists());
            gone.store(true, Ordering::SeqCst);
        });
        launcher.run_until(&spec, &|| gone.load(Ordering::SeqCst))
    });
    assert_eq!(result.exit, None);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_whose_work_is_stopped_is_ended_and_its_cores_are_given_back() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use ignite_core::capacity::Floor;
    use ignite_core::time::SystemClock;
    use ignite_daemon::sched::{Limits, Scheduler};
    use ignite_daemon::steps::{Kind, Steps, Work};

    struct IdleHost;
    impl ignite_os::hostcpu::HostCpu for IdleHost {
        fn cores(&self) -> u32 {
            6
        }
        fn busy(&self) -> std::io::Result<f64> {
            Ok(0.0)
        }
        fn free_memory_mb(&self) -> std::io::Result<u64> {
            Err(std::io::Error::other("not read"))
        }
    }

    let root = scratch("stop");
    let started = root.join("started");
    let mut spec = fixture::waiting(&root, &root.join("never"), 0);
    spec.cores = 3;
    let scheduler = Arc::new(Scheduler::new(
        Limits {
            cores: 6,
            reserved: 0,
            tip_build: 0,
            floor: Floor::default(),
            resources: BTreeMap::new(),
        },
        Arc::new(SystemClock),
        Arc::new(IdleHost),
    ));
    let launcher = launcher(&root).with_scheduler(Arc::clone(&scheduler));
    let steps = Steps::new(Arc::new(SystemClock));
    let test = steps.begin(Kind::Verb("test".to_owned()), Work::default());
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            fixture::until(|| started.exists());
            assert_eq!(scheduler.status().granted, 3);
            test.request_stop().unwrap();
        });
        let _entered = test.enter();
        launcher.run(&spec)
    });
    assert_eq!(result.exit, None);
    assert_eq!(scheduler.status().granted, 0);
    // Nothing more starts for work that was asked to stop.
    let _entered = test.enter();
    let again = launcher.run(&spec);
    assert_eq!(again.exit, None);
    assert_eq!(again.output, PathBuf::new(), "no supervisor was started");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_outlives_the_daemon_side_and_its_result_is_picked_up_afterwards() {
    let root = scratch("restart");
    let go = root.join("go");
    // The command waits for the test to create `go`, so it is certainly still running when
    // the daemon side has gone.
    let spec = fixture::waiting(&root, &go, 0);
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
    let output = fs::read_to_string(&result.output).unwrap();
    assert!(output.lines().any(|line| line == "finished"), "{output}");
    drop(lock);
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
    let (spec, listener) = fixture::with_descendant(&root, true);
    let started = launcher(&root).start(&spec, Vec::new(), 0.0).unwrap();
    let run = started.run().clone();
    assert!(run.dir().join("group").exists());
    fixture::until(|| root.join("descendant-started").exists());
    run.end().unwrap();
    let result = started.wait();
    assert_eq!(result.exit, None);
    fixture::assert_disconnected(&listener);
    fs::remove_dir_all(&root).unwrap();
}

mod grants {
    use std::collections::BTreeMap;
    use std::io;
    #[cfg(unix)]
    use std::process::Command;
    use std::sync::Arc;
    use std::thread;

    use ignite_core::capacity::Floor;
    use ignite_core::fakes::FakeClock;
    use ignite_core::time::Timestamp;
    use ignite_daemon::sched::{GrantRequest, Limits, Scheduler};
    use ignite_os::hostcpu::HostCpu;

    use super::*;

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
            reserved: 0,
            tip_build: 0,
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
            reserve: None,
            priority_ticket: None,
        }
    }

    #[cfg(unix)]
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
        let mut spec = fixture::spec(&root, "watch-cores");
        spec.cores = 4;
        spec.grows = true;
        let started = root.join("started");
        let launcher = launcher(&root).with_scheduler(Arc::clone(&scheduler));
        let run = thread::spawn(move || launcher.run(&spec));

        fixture::until(|| started.exists());
        scheduler.release(&holder);
        let result = run.join().unwrap();

        assert_eq!(result.exit, Some(0));
        let output = fs::read_to_string(&result.output).unwrap();
        assert!(output.lines().any(|line| line == "first 2"), "{output}");
        assert!(output.lines().any(|line| line == "last 4"), "{output}");
        fs::remove_dir_all(&root).unwrap();
    }

    /// A Cargo build that only compiles runs under the supervisor's jobserver, with
    /// `CARGO_BUILD_JOBS` at the most one command may hold. Its job slots grow when another grant is released, and it gives cores
    /// back while a landing waits, so the landing gets them before the build ends.
    // These builds take slots from the POSIX FIFO used by the jobserver.
    #[cfg(unix)]
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
    [ "$CARGO_BUILD_JOBS" = 4 ] || { echo "CARGO_BUILD_JOBS is $CARGO_BUILD_JOBS"; exit 1; }
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
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        }
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

    /// A build started while the machine was busy stayed on its first core after the machine
    /// freed up: only a command whose first word was `cargo` got a jobserver, and the `b3d`
    /// product runs Cargo from a Python script. Now a script that starts Cargo inside it is
    /// granted one core, and once another grant is released its Cargo takes three more job
    /// slots. Its tests still run on the grant's first size.
    // These builds take slots from the POSIX FIFO used by the jobserver.
    #[cfg(unix)]
    #[test]
    fn cargo_inside_a_script_takes_job_slots_as_the_grant_grows() {
        let root = scratch("script");
        let scheduler = six_cores();
        let holder = scheduler.acquire(&ask(Class::Agent, 4));
        let other = scheduler.acquire(&ask(Class::Agent, 1));
        let started = signal(&root, "started");
        // A stand-in for Cargo that takes slots from the jobserver as compiler jobs would: the
        // read blocks until the FIFO holds three bytes.
        let cargo = root.join("cargo");
        fs::write(
            &cargo,
            r#"#!/bin/sh
    echo "jobs $CARGO_BUILD_JOBS, tests $NEXTEST_TEST_THREADS"
    echo go > "$STARTED"
    [ -n "$CARGO_MAKEFLAGS" ] || { echo "no jobserver"; exit 1; }
    exec 3<>"${CARGO_MAKEFLAGS#*fifo:}"
    dd bs=1 count=3 <&3 >/dev/null 2>&1
    echo "took 3 slots"
    printf '+++' >&3
    "#,
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut spec = shell(
            &root,
            r#"echo "script starts"; "$CARGO" build; echo "script ends""#,
        );
        spec.cores = 4;
        spec.env.extend([
            ("STARTED".to_owned(), started.display().to_string()),
            ("CARGO".to_owned(), cargo.display().to_string()),
        ]);
        let launcher = launcher(&root).with_scheduler(Arc::clone(&scheduler));
        let run = thread::spawn(move || launcher.run(&spec));

        fs::read_to_string(&started).unwrap();
        scheduler.release(&holder);
        let result = run.join().unwrap();
        scheduler.release(&other);

        let output = fs::read_to_string(&result.output).unwrap();
        assert_eq!(result.exit, Some(0), "{output}");
        assert!(output.contains("jobs 4, tests 1\n"), "{output}");
        assert!(output.contains("took 3 slots\nscript ends\n"), "{output}");
        fs::remove_dir_all(&root).unwrap();
    }
}

#[test]
fn a_command_past_its_hang_limit_is_ended_and_reads_as_hung() {
    let root = scratch("hung");
    // On Unix the child ignores SIGTERM to exercise the watchdog's SIGKILL fallback.
    #[cfg(unix)]
    let mut spec = shell(
        &root,
        "/bin/sh -c 'trap \"\" TERM; while :; do sleep 1; done' & wait",
    );
    #[cfg(windows)]
    let mut spec = fixture::waiting(&root, &root.join("never"), 0);
    spec.hang_limit_s = Some(1);
    let result = launcher(&root).run(&spec);
    assert!(result.hung);
    assert_eq!(result.exit, None);
    let output = fs::read_to_string(&result.output).unwrap();
    assert!(output.contains("hang limit of 1 s"), "{output}");
    #[cfg(unix)]
    assert!(
        output.contains("while :"),
        "the processes left are listed: {output}"
    );
    #[cfg(windows)]
    {
        let group = fs::read_to_string(result.output.with_file_name("group")).unwrap();
        assert!(
            output
                .lines()
                .any(|line| line.split_whitespace().next() == Some(group.trim())),
            "the watchdog lists the command's PID: {output}"
        );
    }
    let file: ResultFile = serde_json::from_slice(
        &fs::read(result.output.parent().unwrap().join("result.json")).unwrap(),
    )
    .unwrap();
    assert!(file.hung);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_command_within_its_hang_limit_is_not_hung() {
    let root = scratch("in-time");
    let mut spec = shell(&root, "exit 3");
    spec.hang_limit_s = Some(600);
    let result = launcher(&root).run(&spec);
    assert_eq!((result.exit, result.hung), (Some(3), false));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn old_run_directories_are_deleted_and_judged_ones_are_kept_longer() {
    use ignite_daemon::supervise::{KEEP_JUDGED_S, KEEP_S, keep};
    use std::time::{Duration, SystemTime};

    let root = scratch("prune");
    let launcher = launcher(&root);
    let plain = launcher.run(&shell(&root, "echo plain"));
    let judged = launcher.run(&shell(&root, "echo judged"));
    keep(&judged.output).unwrap();
    let alive = |result: &ignite_core::traits::RunResult| result.output.exists();
    let after = |seconds: u64| SystemTime::now() + Duration::from_secs(seconds);

    assert_eq!(launcher.prune(after(KEEP_S / 2)), 0);
    assert_eq!(launcher.prune(after(KEEP_S + 60)), 1);
    assert!(!alive(&plain));
    assert!(alive(&judged));
    assert_eq!(launcher.prune(after(KEEP_JUDGED_S + 60)), 1);
    assert!(!alive(&judged));
    fs::remove_dir_all(&root).unwrap();
}

/// An `idle` command that ignores `SIGTERM` when the scheduler stops it for a landing is killed
/// once the grace has passed.
#[cfg(unix)]
mod stop_grace {
    use std::collections::BTreeMap;
    use std::io;
    use std::process::Command;
    use std::sync::Arc;

    use ignite_core::capacity::Floor;
    use ignite_core::time::SystemClock;
    use ignite_core::traits::Clock as _;
    use ignite_daemon::sched::{GrantRequest, Limits, Scheduler};
    use ignite_os::hostcpu::HostCpu;

    use super::*;

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

    #[test]
    fn idle_work_that_ignores_sigterm_is_killed_after_the_grace() {
        let root = scratch("stop-grace");
        let scheduler = Arc::new(Scheduler::new(
            Limits {
                cores: 6,
                reserved: 0,
                tip_build: 0,
                floor: Floor::default(),
                resources: BTreeMap::from([("pages".to_owned(), 1)]),
            },
            Arc::new(SystemClock),
            Arc::new(IdleHost),
        ));
        let started = root.join("started");
        assert!(
            Command::new("mkfifo")
                .arg(&started)
                .status()
                .unwrap()
                .success()
        );
        let mut spec = shell(
            &root,
            r#"trap '' TERM; echo go > "$STARTED"; exec /bin/sleep 86400"#,
        );
        spec.env
            .push(("STARTED".to_owned(), started.display().to_string()));
        spec.class = Class::Idle;
        spec.cores = 4;
        spec.resources = vec!["pages".to_owned()];
        let launcher = Arc::new(
            launcher(&root)
                .with_scheduler(Arc::clone(&scheduler))
                .with_stop_grace(1.0),
        );
        let run = launcher
            .begin(&spec, SystemClock.now(), &|| false)
            .unwrap()
            .unwrap();
        assert!(
            run.join("group").is_file(),
            "the supervisor recorded the process group"
        );
        // The shell sends this after installing the signal disposition inherited by sleep.
        assert_eq!(fs::read_to_string(&started).unwrap(), "go\n");
        let ask = |class, cores, resources| GrantRequest {
            class,
            cores,
            resources,
            memory_mb: None,
            shrinks: false,
            grows: false,
            cores_file: None,
            reserve: None,
            priority_ticket: None,
        };
        // An agent takes the two cores the idle command left, so a landing has no cores free
        // and the scheduler stops the idle command for it.
        let agent = scheduler.acquire(&ask(Class::Agent, 2, Vec::new()));
        let land = scheduler.acquire(&ask(Class::Land, 4, spec.resources));
        scheduler.release(&agent);
        scheduler.release(&land);
        // The scheduler hands the landing its grant after collecting the idle command's result.
        assert!(run.join("ended").is_file(), "a graceful stop was requested");
        let result: ResultFile =
            serde_json::from_slice(&fs::read(run.join("result.json")).unwrap()).unwrap();
        assert_eq!(result.exit, None, "the command was ended by a signal");
        assert!(!result.hung);
        fs::remove_dir_all(&root).unwrap();
    }
}
