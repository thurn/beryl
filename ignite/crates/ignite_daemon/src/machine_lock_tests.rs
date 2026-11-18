use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

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

#[test]
fn a_second_daemon_is_refused_until_the_first_dies() {
    let root = std::env::temp_dir().join(format!("ignite-daemon-lock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = Home { root: root.clone() };

    let mut first = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "machine_lock::tests::hold_until_killed",
            "--ignored",
            "--nocapture",
        ])
        .env(HOME_VARIABLE, &root)
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
    let path = root.join(LOCK_FILE);
    let before = identity(&path);

    let refused = hold(&home).unwrap_err();
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
