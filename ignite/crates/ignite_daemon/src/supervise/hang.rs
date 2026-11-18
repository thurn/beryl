//! The hang limit: a supervisor's watchdog that ends a command still running at its limit,
//! after writing the processes left in its group to its output.

use std::fs::File;
use std::io::{self, Write};
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ignite_os::Native;

/// How long a hung command has to exit after `SIGTERM` before its group gets `SIGKILL`.
const HANG_GRACE: Duration = Duration::from_secs(5);

/// Ends a command's group once it runs past its hang limit.
pub(super) struct Watchdog {
    /// Dropped once the command has exited.
    exited: mpsc::Sender<()>,
    thread: thread::JoinHandle<bool>,
}

impl Watchdog {
    /// Watches the group `group`, writing to `output` the processes left in it if it is still
    /// running after `limit_s` seconds, then ending them.
    pub(super) fn start(group: u64, limit_s: u64, mut output: File) -> Self {
        let (exited, watched) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            let limit = Duration::from_secs(limit_s);
            if !matches!(
                watched.recv_timeout(limit),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                return false;
            }
            let _ = writeln!(
                output,
                "\nignite: hung: still running after its hang limit of {limit_s} s; ending \
                 these processes:\n{}",
                group_processes(group)
            );
            let _ = signal_group(group, "TERM");
            // Whatever ignored `SIGTERM` is killed, whether or not the leader has exited.
            let _ = watched.recv_timeout(HANG_GRACE);
            let _ = signal_group(group, "KILL");
            true
        });
        Self { exited, thread }
    }

    /// Stops watching an exited command; true if it was ended for hanging.
    pub(super) fn finish(self) -> bool {
        drop(self.exited);
        self.thread.join().unwrap_or(false)
    }
}

/// The processes in the group, one `<pid> <command>` per line, as `ps` prints them. Windows
/// has no `ps`, so there the line holds the process ID alone.
fn group_processes(group: u64) -> String {
    let pids = match Native.group_members(group) {
        Ok(pids) if !pids.is_empty() => pids,
        Ok(_) => return "(none)".to_owned(),
        Err(error) => return format!("(cannot list them: {error})"),
    };
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    describe(&list)
}

#[cfg(unix)]
fn describe(pids: &[String]) -> String {
    Command::new("/bin/ps")
        .args(["-o", "pid=,command=", "-p", &pids.join(",")])
        .output()
        .map_or_else(
            |error| format!("(cannot list them: {error})"),
            |out| String::from_utf8_lossy(&out.stdout).into_owned(),
        )
}

#[cfg(windows)]
fn describe(pids: &[String]) -> String {
    pids.join("\n")
}

/// Sends `SIGTERM` or `SIGKILL` (`signal` is `TERM` or `KILL`) to every process in the group.
#[cfg(unix)]
pub(super) fn signal_group(group: u64, signal: &str) -> io::Result<()> {
    Command::new("/bin/kill")
        .args([&format!("-{signal}"), "--", &format!("-{group}")])
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

/// Ends every process in the group's job object. Windows has no request to exit that every
/// process answers, so `TERM` ends the processes at once, as `KILL` does.
#[cfg(windows)]
pub(super) fn signal_group(group: u64, _signal: &str) -> io::Result<()> {
    Native.end_group_id(group)
}
