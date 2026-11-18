//! What a supervised command leaves behind, and what a restarted daemon does with a command that
//! is still running. macOS only: the scripts need a POSIX shell and process groups.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

use ignite_core::config::Floor;
use ignite_core::fakes::FakeClock;
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Launcher};
use ignite_daemon::sched::{Limits, Scheduler};
use ignite_daemon::supervise::{GrantFile, SpecFile, SupervisedLauncher};
use ignite_os::hostcpu::HostCpu;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-cleanup-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn launcher(root: &Path) -> SupervisedLauncher {
    SupervisedLauncher::new(Path::new(env!("CARGO_BIN_EXE_ignite")), root.join("runs"))
}

fn alive(pid: i32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// A process that has been killed stays a zombie until its parent reaps it, so "alive" also
/// asks `ps` for its state.
fn running(pid: i32) -> bool {
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8_lossy(&out.stdout);
    alive(pid) && !state.trim().is_empty() && !state.trim().starts_with('Z')
}

#[test]
fn a_process_a_command_leaves_in_the_background_ends_with_the_command() {
    let root = scratch("leftover");
    let pid_file = root.join("pid");
    let spec = CommandSpec {
        args: vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            format!(
                "sleep 1000 >/dev/null 2>&1 & echo $! > '{}'",
                pid_file.display()
            ),
        ],
        cwd: root.clone(),
        env: Vec::new(),
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
    };
    let result = launcher(&root).run(&spec);
    assert_eq!(result.exit, Some(0));
    let pid: i32 = fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!running(pid), "the background process outlived its command");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_command_of_a_supervisor_that_died_is_killed_when_its_run_is_collected() {
    let root = scratch("orphan");
    let runs = root.join("runs");
    let run_dir = runs.join("00000000000000000001-1-0");
    fs::create_dir_all(&run_dir).unwrap();
    // A command in a group of its own whose supervisor is gone: no lock is held, no result exists.
    let orphan = Command::new("/bin/sleep")
        .arg("1000")
        .stdout(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = i32::try_from(orphan.id()).unwrap();
    fs::write(run_dir.join("group"), pid.to_string()).unwrap();
    let spec = spec_file(None);
    fs::write(
        run_dir.join("spec.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();

    let runs = launcher(&root).unfinished();
    assert_eq!(runs.len(), 1);
    let result = runs[0].wait();
    assert_eq!(result.exit, None);
    let mut orphan = orphan;
    // Killed, so reaping it returns at once with a signal.
    assert!(orphan.wait().unwrap().code().is_none());
    fs::remove_dir_all(&root).unwrap();
}

fn spec_file(grant: Option<GrantFile>) -> SpecFile {
    SpecFile {
        args: vec!["/bin/true".to_owned()],
        cwd: PathBuf::from("/"),
        env: Vec::new(),
        queued_s: 0.0,
        jobserver: None,
        build_jobs: None,
        hang_limit_s: None,
        grant,
    }
}

struct IdleHost;

impl HostCpu for IdleHost {
    fn cores(&self) -> u32 {
        8
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(0.0)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Err(io::Error::other("not read"))
    }
}

#[test]
fn a_restarted_daemon_registers_the_grants_of_commands_still_running() {
    let root = scratch("adopt");
    let run_dir = root.join("runs").join("00000000000000000001-1-0");
    fs::create_dir_all(&run_dir).unwrap();
    let grant = GrantFile {
        class: Class::Agent,
        cores: 3,
        resources: vec!["oracle".to_owned()],
        asked: None,
        follows: false,
        shrinks: false,
    };
    fs::write(
        run_dir.join("spec.json"),
        serde_json::to_vec(&spec_file(Some(grant))).unwrap(),
    )
    .unwrap();
    // The old supervisor's lock, held until the test lets the command "finish".
    let lock = File::create(run_dir.join("lock")).unwrap();
    lock.lock().unwrap();

    let limits = Limits {
        cores: 8,
        floor: Floor::default(),
        resources: BTreeMap::from([("oracle".to_owned(), 1)]),
    };
    let scheduler = Arc::new(Scheduler::new(
        limits,
        Arc::new(FakeClock::new(Timestamp::EPOCH)),
        Arc::new(IdleHost),
    ));
    let launcher = launcher(&root).with_scheduler(scheduler.clone());
    launcher.adopt_unfinished();
    let status = scheduler.status();
    assert_eq!(status.granted, 3);
    let oracle = status
        .resources
        .iter()
        .find(|r| r.name == "oracle")
        .unwrap();
    assert_eq!(oracle.held, 1);

    // The command ends: its supervisor's lock is released and the grant with it.
    drop(lock);
    while scheduler.status().granted != 0 {
        thread::yield_now();
    }
    fs::remove_dir_all(&root).unwrap();
}

/// A command whose jobserver followed its grant's cores file stayed at its size after a daemon
/// restart: the restarted daemon registered the grant without the file, so it never grew again.
/// An adopted grant that followed its file keeps growing to what the command asked for.
#[test]
fn an_adopted_grant_keeps_growing_through_its_cores_file() {
    let root = scratch("adopt-grows");
    let run_dir = root.join("runs").join("00000000000000000001-1-0");
    fs::create_dir_all(&run_dir).unwrap();
    let grant = GrantFile {
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        asked: Some(4),
        follows: true,
        shrinks: true,
    };
    fs::write(
        run_dir.join("spec.json"),
        serde_json::to_vec(&spec_file(Some(grant))).unwrap(),
    )
    .unwrap();
    fs::write(run_dir.join("cores"), "1").unwrap();
    let lock = File::create(run_dir.join("lock")).unwrap();
    lock.lock().unwrap();

    let limits = Limits {
        cores: 12,
        floor: Floor::default(),
        resources: BTreeMap::new(),
    };
    let scheduler = Arc::new(Scheduler::new(
        limits,
        Arc::new(FakeClock::new(Timestamp::EPOCH)),
        Arc::new(IdleHost),
    ));
    let launcher = launcher(&root).with_scheduler(scheduler.clone());
    launcher.adopt_unfinished();

    assert_eq!(scheduler.status().granted, 4);
    let file = fs::read_to_string(run_dir.join("cores")).unwrap();
    assert_eq!(file.trim(), "4");

    drop(lock);
    while scheduler.status().granted != 0 {
        thread::yield_now();
    }
    fs::remove_dir_all(&root).unwrap();
}
