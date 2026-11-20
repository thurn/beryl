use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use super::*;

const CHILD: &str = "IGNITE_TERMINATE_TEST_MARKER";

/// Runs itself again as a child that installs the handler and sends itself `SIGTERM`: the step
/// runs, and the child still ends by `SIGTERM`, as a daemon without the handler did.
#[test]
fn a_stop_runs_the_last_step_and_still_ends_by_sigterm() {
    if let Some(marker) = std::env::var_os(CHILD) {
        on_terminate(Box::new(move || {
            std::fs::write(&marker, "stopped").unwrap();
        }))
        .unwrap();
        // SAFETY: sends a signal to this process.
        unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        std::thread::sleep(std::time::Duration::from_secs(30));
        panic!("SIGTERM did not end the process");
    }
    let marker = ignite_core::test_files::temp_dir().join(format!("ignite-terminate-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "terminate::tests::a_stop_runs_the_last_step_and_still_ends_by_sigterm",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, &marker)
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "stopped");
    let _ = std::fs::remove_file(&marker);
}
