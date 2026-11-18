//! The macOS half of `usage`.
//!
//! macOS keeps a resource record for every process, which `proc_pid_rusage` reads: CPU time,
//! instructions retired and the largest memory footprint the process ever had. A finished
//! child's record is read after `waitid(..., WNOWAIT)` reports its exit and before it is
//! reaped, while the zombie still holds it. That is how `/usr/bin/time -l` gets the same
//! figures.
//!
//! The record covers one process. Its child fields (`ri_child_*`, in every version of the
//! record) total CPU time, wakeups and page-ins of the children it reaped, but have no field
//! for their instructions, so the leader's record does not hold its descendants' instructions.
//! So for a command that starts processes of its own, [`TreeMeter`] reads every process in the
//! command's process group a few times a second while it runs and
//! keeps the last reading of each one that goes away. A descendant that starts and exits between
//! two readings is not counted at all; see "Cost in instructions" in `docs/ignite.md`.

use std::collections::HashMap;
use std::io;
use std::mem::MaybeUninit;
use std::process::{Child, ExitStatus};
use std::sync::OnceLock;

use super::{ProcessUsage, UsageReader};
use crate::Native;

impl UsageReader for Native {
    fn sample(&self, pid: u32) -> io::Result<ProcessUsage> {
        let pid = to_pid(pid)?;
        // SAFETY: `getpgid` only reads its integer argument.
        let group = unsafe { libc::getpgid(pid) };
        if group < 0 {
            return Err(io::Error::last_os_error());
        }
        // The descendants are the rest of the process group that `pid` leads, as
        // `Processes::spawn_group` starts commands. A process that leads no group is read alone,
        // so its group's other processes, which it did not start, are not counted.
        let members = if group == pid {
            self.group_members(group as u64)?
        } else {
            vec![pid as u32]
        };
        let mut total = ProcessUsage {
            instructions: Some(0),
            ..ProcessUsage::default()
        };
        for member in members {
            // A process that exits between the listing and the reading is skipped.
            if let Ok(reading) = read(member) {
                // While processes run, the tree's memory is what they hold now.
                let usage = ProcessUsage {
                    peak_bytes: reading.current_bytes,
                    ..reading.usage
                };
                add(&mut total, usage);
            }
        }
        Ok(total)
    }

    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)> {
        let pid = to_pid(child.id())?;
        wait_without_reaping(pid)?;
        let reading = read(pid as u32);
        // Reap it even if the reading failed, so no zombie is left behind.
        let status = child.wait()?;
        Ok((status, reading?.usage))
    }
}

/// The resource use of a command's whole process group: the descendants it started as well
/// as the command itself.
///
/// Call [`TreeMeter::sample`] about once a second while the command runs, then
/// [`TreeMeter::finish`] with the leader's exact use from [`UsageReader::reap`]. A
/// descendant that exits between two samples is counted up to the earlier one, and one that
/// leaves the group with `setsid` is not counted at all.
#[derive(Debug, Default)]
pub struct TreeMeter {
    group: u64,
    leader: u32,
    /// The last reading of each descendant, keyed by its PID and start time so that a reused
    /// PID counts as a new process.
    last: HashMap<(u32, u64), ProcessUsage>,
    /// The most memory the whole group held at one sample.
    peak_bytes: u64,
}

impl TreeMeter {
    /// Meters the process group `group`, whose leader `leader` is read exactly when reaped.
    pub fn new(group: u64, leader: u32) -> Self {
        TreeMeter {
            group,
            leader,
            ..TreeMeter::default()
        }
    }

    /// Reads every process now in the group.
    pub fn sample(&mut self) -> io::Result<()> {
        let mut held = 0;
        for pid in Native.group_members(self.group)? {
            let Ok(reading) = read(pid) else { continue };
            held += reading.current_bytes;
            if pid != self.leader {
                self.last.insert((pid, reading.start), reading.usage);
            }
        }
        self.peak_bytes = self.peak_bytes.max(held);
        Ok(())
    }

    /// The group's total: `leader`'s final use plus the last reading of every descendant.
    pub fn finish(&self, leader: ProcessUsage) -> ProcessUsage {
        let mut total = leader;
        for usage in self.last.values() {
            add(&mut total, *usage);
        }
        total.peak_bytes = self.peak_bytes.max(leader.peak_bytes);
        total
    }
}

struct Reading {
    usage: ProcessUsage,
    /// The memory footprint now, where `usage.peak_bytes` is the largest it has been.
    current_bytes: u64,
    /// When the process started, in Mach absolute time.
    start: u64,
}

fn read(pid: u32) -> io::Result<Reading> {
    let pid = to_pid(pid)?;
    let mut info = MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: `info` is large enough for the version-4 record that `RUSAGE_INFO_V4` asks for.
    let result =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, info.as_mut_ptr().cast()) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so it filled the record; it was zeroed before in any case.
    let info = unsafe { info.assume_init() };
    Ok(Reading {
        usage: ProcessUsage {
            cpu_s: seconds(info.ri_user_time + info.ri_system_time),
            instructions: Some(info.ri_instructions),
            peak_bytes: info.ri_lifetime_max_phys_footprint,
        },
        current_bytes: info.ri_phys_footprint,
        start: info.ri_proc_start_abstime,
    })
}

fn add(total: &mut ProcessUsage, usage: ProcessUsage) {
    total.cpu_s += usage.cpu_s;
    total.instructions = match (total.instructions, usage.instructions) {
        (Some(a), Some(b)) => Some(a + b),
        _ => None,
    };
    total.peak_bytes += usage.peak_bytes;
}

/// Converts the record's CPU times, which are in Mach absolute time units, to seconds.
#[allow(deprecated)] // `libc` points to another crate for the timebase; the call is the same.
fn seconds(ticks: u64) -> f64 {
    static TIMEBASE: OnceLock<(u32, u32)> = OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid, writable timebase record.
        let result = unsafe { libc::mach_timebase_info(&raw mut info) };
        if result == 0 && info.denom != 0 {
            (info.numer, info.denom)
        } else {
            (1, 1)
        }
    });
    ticks as f64 * f64::from(numer) / f64::from(denom) / 1e9
}

/// Blocks until `pid` has exited, leaving it unreaped so its record can still be read.
fn wait_without_reaping(pid: libc::pid_t) -> io::Result<()> {
    loop {
        let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
        // SAFETY: `info` is a valid, writable `siginfo_t`.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn to_pid(pid: u32) -> io::Result<libc::pid_t> {
    libc::pid_t::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a process ID"))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    use super::*;
    use crate::proc::Processes;

    /// A shell loop of a fixed number of iterations: the same instructions on every run.
    const WORK: &str = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done";

    fn run_work() -> ProcessUsage {
        let mut child = Command::new("/bin/sh").args(["-c", WORK]).spawn().unwrap();
        let (status, usage) = Native.reap(&mut child).unwrap();
        assert_eq!(status.code(), Some(0));
        usage
    }

    #[test]
    fn a_fixed_amount_of_work_reports_the_same_instructions() {
        let first = run_work();
        let second = run_work();
        let (a, b) = (
            first.instructions.unwrap() as f64,
            second.instructions.unwrap() as f64,
        );
        assert!(a > 10_000_000.0, "too few instructions: {a}");
        let difference = (a - b).abs() / a.max(b);
        assert!(
            difference < 0.02,
            "{a} and {b} differ by {:.2}%",
            difference * 100.0
        );
        assert!(first.cpu_s > 0.0);
        assert!(first.peak_bytes > 0);
    }

    #[test]
    fn reaping_returns_the_exit_code() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .unwrap();
        let (status, _) = Native.reap(&mut child).unwrap();
        assert_eq!(status.code(), Some(7));
        // The child was reaped: waiting again returns the same status without blocking.
        assert_eq!(child.try_wait().unwrap(), Some(status));
    }

    #[test]
    fn the_tree_meter_counts_a_descendant() {
        // The subshell is a separate process: it does the work, says so, then waits for its
        // input to close, which keeps it alive until the meter has read it.
        let script = format!("( {WORK}; echo done; read line ); true");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        let mut meter = TreeMeter::new(group.id, group.child.id());
        let mut lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        assert_eq!(lines.next().unwrap().unwrap(), "done");

        meter.sample().unwrap();
        let running = Native.sample(group.child.id()).unwrap();
        assert!(running.instructions.unwrap() > 10_000_000);

        let mut stdin = group.child.stdin.take().unwrap();
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
        let (status, leader) = Native.reap(&mut group.child).unwrap();
        assert!(status.success());

        let total = meter.finish(leader);
        let descendants = total.instructions.unwrap() - leader.instructions.unwrap();
        assert!(
            descendants > 10_000_000,
            "descendant counted {descendants} instructions"
        );
        assert!(total.cpu_s > leader.cpu_s);
    }
}
