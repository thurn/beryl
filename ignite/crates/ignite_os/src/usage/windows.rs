//! The Windows half of `usage`.
//!
//! Windows counts no instructions, so every reading here has `instructions: None`. A process's
//! CPU time comes from `GetProcessTimes` and its memory from `K32GetProcessMemoryInfo`. Both
//! still answer after the process has exited, while a handle to it is open, as `Child` keeps
//! one until it is dropped.
//!
//! A command started by `Processes::spawn_group` runs in a job object, whose accounting totals
//! the CPU time of every process the job ever held. So [`TreeMeter`] reads the tree's CPU time
//! exactly, and samples only the memory its processes hold.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::{Child, ExitStatus};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

use super::{ProcessUsage, UsageReader};
use crate::Native;
use crate::proc::windows::{JOB_OBJECT_QUERY, Job};

impl UsageReader for Native {
    fn sample(&self, pid: u32) -> io::Result<ProcessUsage> {
        // A process that leads a group is read with the rest of its job; any other alone.
        if let Some(job) = Job::open(u64::from(pid), JOB_OBJECT_QUERY)? {
            return Ok(ProcessUsage {
                cpu_s: job_cpu_s(&job)?,
                instructions: None,
                peak_bytes: held_bytes(&job)?,
            });
        }
        let process = Process::open(pid)?;
        let reading = read(process.0)?;
        // While the process runs, its memory is what it holds now.
        Ok(ProcessUsage {
            peak_bytes: reading.current_bytes,
            ..reading.usage
        })
    }

    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)> {
        let status = child.wait()?;
        let reading = read(child.as_raw_handle())?;
        Ok((status, reading.usage))
    }
}

/// The resource use of a command's whole job: the descendants it started as well as the
/// command itself.
///
/// Call [`TreeMeter::sample`] about once a second while the command runs, then
/// [`TreeMeter::finish`] with the leader's use from [`UsageReader::reap`]. The CPU time is
/// exact, from the job's accounting; the peak memory is the most the job's processes held at
/// one sample.
#[derive(Debug, Default)]
pub struct TreeMeter {
    /// The command's job, or `None` if it could not be opened, as for a command started
    /// outside a group.
    job: Option<Job>,
    /// The most memory the whole job held at one sample.
    peak_bytes: u64,
}

impl TreeMeter {
    /// Meters the job of group `group`. `leader` is the group's first process, whose ID on
    /// Windows is the group's.
    pub fn new(group: u64, leader: u32) -> Self {
        debug_assert_eq!(group, u64::from(leader));
        TreeMeter {
            job: Job::open(group, JOB_OBJECT_QUERY).ok().flatten(),
            peak_bytes: 0,
        }
    }

    /// Reads the memory every process in the job holds now.
    pub fn sample(&mut self) -> io::Result<()> {
        if let Some(job) = &self.job {
            self.peak_bytes = self.peak_bytes.max(held_bytes(job)?);
        }
        Ok(())
    }

    /// The job's total: the CPU time of every process it held, and the most memory it held.
    /// Without a job, `leader`'s use alone.
    pub fn finish(&self, leader: ProcessUsage) -> ProcessUsage {
        let cpu_s = self
            .job
            .as_ref()
            .and_then(|job| job_cpu_s(job).ok())
            .map_or(leader.cpu_s, |total| total.max(leader.cpu_s));
        ProcessUsage {
            cpu_s,
            instructions: None,
            peak_bytes: self.peak_bytes.max(leader.peak_bytes),
        }
    }
}

/// An open process handle, closed when dropped.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> io::Result<Process> {
        // SAFETY: plain call; the handle is closed when the value drops.
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Process(handle))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by this value.
        unsafe { CloseHandle(self.0) };
    }
}

struct Reading {
    usage: ProcessUsage,
    /// The working set now, where `usage.peak_bytes` is the largest it has been.
    current_bytes: u64,
}

fn read(process: HANDLE) -> io::Result<Reading> {
    let (mut creation, mut exit, mut kernel, mut user) = Default::default();
    // SAFETY: the handle is open, and the four pointers are to valid, writable records.
    let ok = unsafe {
        GetProcessTimes(
            process,
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let memory = memory(process)?;
    Ok(Reading {
        usage: ProcessUsage {
            cpu_s: seconds(ticks(kernel) + ticks(user)),
            instructions: None,
            peak_bytes: memory.PeakWorkingSetSize as u64,
        },
        current_bytes: memory.WorkingSetSize as u64,
    })
}

fn memory(process: HANDLE) -> io::Result<PROCESS_MEMORY_COUNTERS> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: the handle is open, and `counters` is a writable record of the size passed.
    let ok = unsafe { K32GetProcessMemoryInfo(process, &raw mut counters, counters.cb) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(counters)
}

/// The CPU time of every process the job ever held.
fn job_cpu_s(job: &Job) -> io::Result<f64> {
    let totals = job.accounting()?;
    let units = totals.TotalUserTime.max(0) + totals.TotalKernelTime.max(0);
    Ok(seconds(units as u64))
}

/// The working sets of the processes in the job now. A process that exits between the listing
/// and the reading is skipped.
fn held_bytes(job: &Job) -> io::Result<u64> {
    Ok(job
        .members()?
        .into_iter()
        .filter_map(|pid| Process::open(pid).ok())
        .filter_map(|process| memory(process.0).ok())
        .map(|counters| counters.WorkingSetSize as u64)
        .sum())
}

/// A `FILETIME`'s two halves as one count of 100-nanosecond units.
fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// 100-nanosecond units as seconds.
fn seconds(units: u64) -> f64 {
    units as f64 / 1e7
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    use super::*;
    use crate::proc::Processes;

    /// The variable that tells [`helper`] what to do.
    const ROLE: &str = "IGNITE_TEST_USAGE_ROLE";

    /// Runs this test binary as one of [`helper`]'s roles.
    fn helper_command(role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "usage::windows::tests::helper", "--ignored"])
            .args(["--nocapture", "--test-threads=1"])
            .env(ROLE, role);
        command
    }

    /// A fixed amount of CPU work.
    fn spin() {
        let mut total = 0u64;
        for i in 0..300_000_000u64 {
            total = std::hint::black_box(total.wrapping_add(i));
        }
    }

    /// Run by the tests below as a separate process. `spin` works, says `done` and waits for a
    /// line on its input; `parent` runs `spin` as its child and waits for it; `exit7` exits
    /// with code 7.
    #[test]
    #[ignore = "run by the other usage tests"]
    fn helper() {
        let Ok(role) = std::env::var(ROLE) else {
            return;
        };
        match role.as_str() {
            "spin" => {
                spin();
                println!("done");
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
            }
            "parent" => {
                let status = helper_command("spin").status().unwrap();
                std::process::exit(status.code().unwrap_or(1));
            }
            "exit7" => std::process::exit(7),
            other => panic!("unknown role {other}"),
        }
        std::process::exit(0);
    }

    #[test]
    fn reaping_returns_the_exit_code_and_cpu_time_but_no_instructions() {
        let mut child = helper_command("spin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let (status, usage) = Native.reap(&mut child).unwrap();
        assert_eq!(status.code(), Some(0));
        assert!(usage.cpu_s > 0.0, "{usage:?}");
        assert!(usage.peak_bytes > 0, "{usage:?}");
        assert_eq!(usage.instructions, None);

        let mut child = helper_command("exit7").spawn().unwrap();
        let (status, _) = Native.reap(&mut child).unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(child.try_wait().unwrap(), Some(status));
    }

    #[test]
    fn the_tree_meter_counts_a_descendant() {
        let mut command = helper_command("parent");
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        let mut meter = TreeMeter::new(group.id, group.child.id());
        let lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let mut lines = lines.map(Result::unwrap);
        // libtest has already printed `test <name> ... ` on the helper's first line, without a
        // newline, so `done` can end a longer line.
        assert!(lines.any(|line| line.ends_with("done")));

        meter.sample().unwrap();
        let running = Native.sample(group.child.id()).unwrap();
        assert!(running.cpu_s > 0.0, "{running:?}");
        assert!(running.peak_bytes > 0, "{running:?}");
        assert_eq!(running.instructions, None);

        let mut stdin = group.child.stdin.take().unwrap();
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
        let (status, leader) = Native.reap(&mut group.child).unwrap();
        assert!(status.success());

        let total = meter.finish(leader);
        assert!(total.cpu_s > leader.cpu_s, "{total:?} {leader:?}");
        assert!(total.peak_bytes > 0);
        assert_eq!(total.instructions, None);
        Native.end_group(&mut group).unwrap();
    }
}
