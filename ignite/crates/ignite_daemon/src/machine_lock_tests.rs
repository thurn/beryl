use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::*;

/// The variable that tells [`hold_until_stdin_closes`] which home to lock.
const HOME_VARIABLE: &str = "IGNITE_TEST_DAEMON_LOCK_HOME";

/// What the first daemon prints once it holds the lock.
const HELD: &str = "first daemon holds the lock";

/// Run by `a_second_daemon_is_refused_until_the_first_dies` as the first daemon: it takes the
/// lock, says so, and holds it until it is killed.
#[test]
#[ignore = "run by a_second_daemon_is_refused_until_the_first_dies"]
fn hold_until_killed() {
    if let Some(root) = std::env::var_os(HOME_VARIABLE) {
        let _lock = hold(&Home { root: root.into() }).unwrap();
        // libtest has already printed `test <name> ... ` on this line.
        println!("{HELD}");
        let _ = std::io::stdin().read_to_end(&mut Vec::new());
    }
}

#[cfg(unix)]
fn identity(path: &std::path::Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

#[cfg(windows)]
fn identity(path: &std::path::Path) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt;
    (fs::metadata(path).unwrap().creation_time(), 0)
}

/// Starts the first daemon in a process of its own, holding the lock in `root`, and returns once
/// it holds it. Closing its standard input ends it.
fn first_daemon(root: &std::path::Path) -> std::process::Child {
    let mut first = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "machine_lock::tests::hold_until_killed",
            "--ignored",
            "--nocapture",
        ])
        .env(HOME_VARIABLE, root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(first.stdout.take().unwrap());
    let mut line = String::new();
    while !line.contains(HELD) {
        line.clear();
        assert_ne!(
            out.read_line(&mut line).unwrap(),
            0,
            "the first daemon exited"
        );
    }
    // Read to the end, so the first daemon can print its test's result when it exits.
    std::thread::spawn(move || std::io::copy(&mut out, &mut std::io::sink()));
    first
}

fn scratch_home(name: &str) -> Home {
    let root = std::env::temp_dir().join(format!("ignite-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    Home { root }
}

#[test]
fn a_second_daemon_is_refused_until_the_first_dies() {
    let home = scratch_home("daemon-lock");
    let root = home.root.clone();
    let mut first = first_daemon(&root);
    let path = root.join(LOCK_FILE);
    let before = identity(&path);

    assert!(held(&home), "a client sees the lock held");
    let refused = hold_waiting(&home, Duration::ZERO, || {}).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AddrInUse);
    assert!(
        refused
            .to_string()
            .contains("another ignite daemon is running"),
        "{refused}"
    );

    first.kill().unwrap();
    first.wait().unwrap();
    let second = hold(&home).unwrap();
    assert_eq!(second.path, path);
    assert_eq!(identity(&path), before, "the lock file was replaced");
    drop(second);
    fs::remove_dir_all(root).unwrap();
}

/// On Windows a restart starts the new daemon before the old one has exited. The new one finds
/// the lock held, waits instead of exiting, and takes the lock once the old one is gone.
#[test]
fn a_restarted_daemon_waits_for_its_predecessor_to_free_the_lock() {
    let home = scratch_home("restart-lock");
    let mut first = first_daemon(&home.root);
    let mut stdin = first.stdin.take();

    let mut found_held = false;
    let lock = hold_waiting(&home, RESTART_WAIT, || {
        found_held = true;
        // The old daemon exits now, while the new one waits.
        drop(stdin.take());
    })
    .unwrap();

    assert!(found_held, "the lock was free from the start");
    assert_eq!(lock.path, home.root.join(LOCK_FILE));
    first.wait().unwrap();
    drop(lock);
    fs::remove_dir_all(&home.root).unwrap();
}

#[test]
fn only_a_restart_from_another_process_waits_long_for_the_lock() {
    assert_eq!(start_wait(Some("4242"), 7), RESTART_WAIT);
    assert_eq!(start_wait(Some("7"), 7), START_GRACE, "macOS keeps the ID");
    assert_eq!(start_wait(Some("garbage"), 7), START_GRACE);
    assert_eq!(start_wait(None, 7), START_GRACE);
}

#[test]
fn a_missing_socket_means_starting_only_after_a_recent_start() {
    let home = scratch_home("lock-starting");
    let now = ignite_core::time::Timestamp::parse("2026-11-18T16:10:40.000Z").unwrap();
    assert!(!daemon_starting(&home, now), "no daemon was ever started");

    let log = crate::update::start_log(&home);
    let commit = ignite_core::ids::Commit::new("a".repeat(40)).unwrap();
    log.record(now.plus_seconds(-2.0), &commit, 1).unwrap();
    assert!(daemon_starting(&home, now), "a start 2 s ago");
    assert!(
        !daemon_starting(&home, now.plus_seconds(RECENT_START_S + 1.0)),
        "a start over a minute ago, and nothing holds the lock"
    );
    fs::remove_dir_all(&home.root).unwrap();
}

/// The home folder holds the daemon's socket and the machine's write token, so only its user
/// may enter it, whether it is new or was made with wider permissions before.
#[cfg(unix)]
#[test]
fn the_home_folder_is_private_to_its_user() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let base = std::env::temp_dir().join(format!("ignite-home-mode-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let mode = |path: &std::path::Path| fs::metadata(path).unwrap().mode() & 0o777;

    let fresh = base.join("fresh");
    let _lock = hold(&Home {
        root: fresh.clone(),
    })
    .unwrap();
    assert_eq!(mode(&fresh), 0o700);

    let old = base.join("old");
    fs::create_dir_all(&old).unwrap();
    fs::set_permissions(&old, fs::Permissions::from_mode(0o755)).unwrap();
    let _lock = hold(&Home { root: old.clone() }).unwrap();
    assert_eq!(mode(&old), 0o700);
    fs::remove_dir_all(&base).unwrap();
}
