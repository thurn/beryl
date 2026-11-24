//! The supervisor's watchdog ends commands at their hang limit and lists the processes left
//! in their group. For checks, the time limit measures how long the output log has been idle.

use std::fs::File;
use std::io::{self, Write};
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ignite_os::Native;

/// A supervisor setting in the command spec's environment, removed before spawning the child.
/// Carrying it with the environment lets older daemons keep the original wall-time limit.
pub(crate) const HANG_ON_OUTPUT_IDLE: &str = "IGNITE_HANG_ON_OUTPUT_IDLE";

/// How often to check whether the command added anything to its output log.
const OUTPUT_SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// How long a hung command has to exit after `SIGTERM` before its group gets `SIGKILL`.
const HANG_GRACE: Duration = Duration::from_secs(5);

/// Ends a command's group once it reaches its hang limit.
pub(super) struct Watchdog {
    /// Dropped once the command has exited.
    exited: mpsc::Sender<()>,
    thread: thread::JoinHandle<bool>,
}

impl Watchdog {
    /// Watches the group and ends it after the limit, measured from startup or, when
    /// `output_idle` is set, since the log last grew.
    pub(super) fn start(group: u64, limit_s: u64, mut output: File, output_idle: bool) -> Self {
        let (exited, watched) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            let limit = Duration::from_secs(limit_s);
            let wait_for_exit = |duration| {
                !matches!(
                    watched.recv_timeout(duration),
                    Err(mpsc::RecvTimeoutError::Timeout)
                )
            };
            let hung = if output_idle {
                let began = Instant::now();
                output_stalled(
                    limit,
                    wait_for_exit,
                    || began.elapsed(),
                    || output.metadata().map(|metadata| metadata.len()),
                )
            } else {
                !wait_for_exit(limit)
            };
            if !hung {
                return false;
            }
            let reason = if output_idle {
                "no new output"
            } else {
                "still running"
            };
            let _ = writeln!(
                output,
                "\nignite: hung: {reason} after its hang limit of {limit_s} s; ending \
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

/// Waits for the command to exit or spend a full limit without writing to the log. The log is
/// written through the child's inherited handles, so only its length needs to be sampled.
fn output_stalled(
    limit: Duration,
    mut wait_for_exit: impl FnMut(Duration) -> bool,
    elapsed: impl Fn() -> Duration,
    mut output_len: impl FnMut() -> io::Result<u64>,
) -> bool {
    let mut deadline = limit;
    let mut last_len = 0;
    loop {
        let remaining = deadline.saturating_sub(elapsed());
        if wait_for_exit(remaining.min(OUTPUT_SAMPLE_EVERY)) {
            return false;
        }
        let sampled_len = output_len();
        let now = elapsed();
        if let Ok(len) = sampled_len
            && len > last_len
        {
            last_len = len;
            deadline = now.saturating_add(limit);
        }
        if now >= deadline {
            // The leader may have exited while we read the log.
            return !wait_for_exit(Duration::ZERO);
        }
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// The child writes a byte per entry in `writes` and exits at `exit_at`. All times
    /// are in seconds, and only the watchdog's waits advance the clock.
    fn observe(writes: &[u64], exit_at: u64) -> (bool, Duration) {
        let clock = Cell::new(Duration::ZERO);
        let exit_at = Duration::from_secs(exit_at);
        let hung = output_stalled(
            Duration::from_secs(5),
            |wait| {
                clock.set((clock.get() + wait).min(exit_at));
                clock.get() == exit_at
            },
            || clock.get(),
            || {
                Ok(writes
                    .iter()
                    .filter(|&&time| Duration::from_secs(time) <= clock.get())
                    .count() as u64)
            },
        );
        (hung, clock.get())
    }

    #[test]
    fn a_check_that_keeps_writing_can_exit_after_its_hang_limit() {
        assert_eq!(observe(&[4, 8, 12], 15), (false, Duration::from_secs(15)));
    }

    #[test]
    fn a_check_that_stops_writing_hits_its_hang_limit_after_the_last_progress() {
        assert_eq!(observe(&[4, 8, 12], 30), (true, Duration::from_secs(17)));
        assert_eq!(observe(&[], 30), (true, Duration::from_secs(5)));
    }
}
