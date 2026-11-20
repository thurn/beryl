//! The macOS half of `usage`.
//!
//! macOS keeps a resource record for every process, which `proc_pid_rusage` reads: CPU time,
//! instructions retired and the largest memory footprint the process ever had. A finished
//! child's record is read after `waitid(..., WNOWAIT)` reports its exit and before it is
//! reaped, while the zombie still holds it. That is how `/usr/bin/time -l` gets the same
//! figures.
//!
//! The record covers one process. Its child fields (`ri_child_*`, in every version of the
//! record) total the CPU time, wakeups and page-ins of the children it reaped, each child with
//! the children it reaped in turn. So a command whose processes all wait for their children
//! leaves its whole tree's CPU time in the leader's record, however short-lived its
//! descendants were. The child fields have no instructions, though, so for a command that
//! starts processes of its own, [`TreeMeter`] reads every process in the command's process
//! group a few times a second while it runs and keeps the last reading of each one that goes
//! away. A descendant that starts and exits between two readings adds no instructions; see
//! "Cost in instructions" in `docs/ignite.md`.

use std::collections::{HashMap, HashSet};
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
                // While processes run, the tree's memory is what they hold now. A running
                // process holds the CPU time of the children it reaped, which are gone.
                let usage = ProcessUsage {
                    peak_bytes: reading.current_bytes,
                    ..reading.with_children_cpu()
                };
                add(&mut total, usage);
            }
        }
        Ok(total)
    }

    /// The CPU time includes the children `child` reaped, and theirs in turn; the instructions
    /// and peak memory are its own.
    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)> {
        let pid = to_pid(child.id())?;
        wait_without_reaping(pid)?;
        let reading = read(pid as u32);
        // Reap it even if the reading failed, so no zombie is left behind.
        let status = child.wait()?;
        Ok((status, reading?.with_children_cpu()))
    }
}

/// The resource use of a command's whole process group: the descendants it started as well
/// as the command itself.
///
/// Call [`TreeMeter::sample`] about once a second while the command runs and once more after
/// reaping the leader, then [`TreeMeter::finish`] with the leader's use from
/// [`UsageReader::reap`].
///
/// The CPU time is the leader's, which holds every descendant that was reaped by the leader or
/// by a descendant that was itself reaped, plus the descendants whose time cannot have reached
/// the leader: those still in the group at the last sample, and those that had lost their
/// parent (launchd reaps them) when last read. A descendant that loses its parent and exits
/// between two samples is not counted, and one that leaves the group with `setsid` is not
/// counted at all.
///
/// The instructions are the leader's own plus the last reading of every descendant, since no
/// record totals a reaped child's instructions. A descendant that exits between two samples
/// adds none.
#[derive(Debug, Default)]
pub struct TreeMeter {
    group: u64,
    leader: u32,
    /// The last reading of each descendant, keyed by its PID and start time so that a reused
    /// PID counts as a new process.
    last: HashMap<(u32, u64), Descendant>,
    /// The descendants in the group at the latest sample.
    present: HashSet<(u32, u64)>,
    /// The most memory the whole group held at one sample.
    peak_bytes: u64,
}

/// A descendant's last reading.
#[derive(Clone, Copy, Debug)]
struct Descendant {
    usage: ProcessUsage,
    children_cpu_s: f64,
    /// Its parent had exited, so launchd, not a process in the tree, will reap it.
    orphaned: bool,
}

impl Descendant {
    /// The CPU seconds it and the children it reaped used.
    fn tree_cpu_s(self) -> f64 {
        self.usage.cpu_s + self.children_cpu_s
    }
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
        // If the group cannot be listed, no descendant is known to be present, so `finish`
        // cannot count one that the leader has already counted.
        self.present.clear();
        let mut held = 0;
        for pid in Native.group_members(self.group)? {
            let Ok(reading) = read(pid) else { continue };
            held += reading.current_bytes;
            if pid != self.leader {
                let key = (pid, reading.start);
                // The process that started it is gone, so its parent is now launchd.
                let orphaned = parent(pid).is_ok_and(|parent| parent == LAUNCHD);
                self.last.insert(
                    key,
                    Descendant {
                        usage: reading.usage,
                        children_cpu_s: reading.children_cpu_s,
                        orphaned,
                    },
                );
                self.present.insert(key);
            }
        }
        self.peak_bytes = self.peak_bytes.max(held);
        Ok(())
    }

    /// The group's total. `leader` is the leader's use from [`UsageReader::reap`], whose CPU
    /// time includes the descendants that were reaped back up to it.
    pub fn finish(&self, leader: ProcessUsage) -> ProcessUsage {
        let mut total = leader;
        for (key, descendant) in &self.last {
            total.instructions = total
                .instructions
                .zip(descendant.usage.instructions)
                .map(|(a, b)| a + b);
            // A descendant still present was not reaped by the leader's tree, and one that lost
            // its parent never will be: their time is not in the leader's record.
            if self.present.contains(key) || descendant.orphaned {
                total.cpu_s += descendant.tree_cpu_s();
            }
        }
        total.peak_bytes = self.peak_bytes.max(leader.peak_bytes);
        total
    }
}

impl Native {
    /// See `HostCpu::tree_cpu_seconds`: every member of process group `group` and, through
    /// their child lists, every process they started that left the group, each counted once
    /// with the time of the children it reaped. A process that exits between the listing and
    /// its reading is skipped.
    pub(crate) fn tree_cpu_seconds_of(self, group: u64) -> io::Result<f64> {
        Ok(descendants_cpu_seconds(
            self.group_members(group)?,
            &mut HashSet::new(),
        ))
    }

    /// See `HostCpu::process_tree_cpu_seconds`: `pid` and every process it started, through any
    /// number of generations, whatever their process groups.
    pub(crate) fn process_tree_cpu_seconds_of(pid: u32) -> f64 {
        descendants_cpu_seconds(vec![pid], &mut HashSet::new())
    }

    /// See `HostCpu::process_trees_cpu_seconds`: each root's tree, with a process that an
    /// earlier root's tree holds counted under that root only. A root that cannot be read, as
    /// one that has exited, is left out.
    pub(crate) fn process_trees_cpu_seconds_of(roots: &[u32]) -> Vec<(u32, f64)> {
        let mut seen = HashSet::new();
        let mut readings = Vec::with_capacity(roots.len());
        for &root in roots {
            if seen.contains(&root) || read(root).is_ok() {
                readings.push((root, descendants_cpu_seconds(vec![root], &mut seen)));
            }
        }
        readings
    }
}

/// The CPU seconds of every process in `start` and, through their child lists, every process
/// they started, each counted once with the time of the children it reaped. A process that
/// exits between the listing and its reading is skipped, and so is a process in `seen`, which
/// gains every process counted.
fn descendants_cpu_seconds(start: Vec<u32>, seen: &mut HashSet<u32>) -> f64 {
    let mut pending = start;
    let mut total = 0.0;
    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            continue;
        }
        let Ok(reading) = read(pid) else { continue };
        total += reading.usage.cpu_s + reading.children_cpu_s;
        pending.extend(children(pid).unwrap_or_default());
    }
    total
}

/// The PID of launchd, which becomes the parent of every process whose parent exits.
const LAUNCHD: u32 = 1;

/// The PID of `pid`'s parent.
fn parent(pid: u32) -> io::Result<u32> {
    let pid = to_pid(pid)?;
    let mut info = MaybeUninit::<libc::proc_bsdshortinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_bsdshortinfo>())
        .map_err(|_| io::Error::other("record too large"))?;
    // SAFETY: `info` is a writable record of `size` bytes, the size the flavor asks for.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call filled the whole record; it was zeroed before in any case.
    Ok(unsafe { info.assume_init() }.pbsi_ppid)
}

/// The PIDs of the processes whose parent is `pid`.
fn children(pid: u32) -> io::Result<Vec<u32>> {
    let pid = to_pid(pid)?;
    let mut capacity = 16;
    loop {
        let mut pids: Vec<libc::pid_t> = vec![0; capacity];
        let bytes = libc::c_int::try_from(capacity * size_of::<libc::pid_t>())
            .map_err(|_| io::Error::other("too many children to list"))?;
        // SAFETY: `pids` is a writable buffer of `bytes` bytes; the call writes at most that.
        let count = unsafe { libc::proc_listchildpids(pid, pids.as_mut_ptr().cast(), bytes) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        // The call returns a count of PIDs; a full buffer may have been cut short.
        let count = count as usize;
        if count < capacity {
            pids.truncate(count);
            return Ok(pids
                .into_iter()
                .filter(|&pid| pid > 0)
                .map(|pid| pid as u32)
                .collect());
        }
        capacity *= 2;
    }
}

struct Reading {
    usage: ProcessUsage,
    /// The memory footprint now, where `usage.peak_bytes` is the largest it has been.
    current_bytes: u64,
    /// When the process started, in Mach absolute time.
    start: u64,
    /// CPU seconds of the children it has reaped.
    children_cpu_s: f64,
}

impl Reading {
    /// Its use with the CPU time of the children it reaped added.
    fn with_children_cpu(&self) -> ProcessUsage {
        ProcessUsage {
            cpu_s: self.usage.cpu_s + self.children_cpu_s,
            ..self.usage
        }
    }
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
        children_cpu_s: seconds(info.ri_child_user_time + info.ri_child_system_time),
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

        meter.sample().unwrap();
        let total = meter.finish(leader);
        let descendants = total.instructions.unwrap() - leader.instructions.unwrap();
        assert!(
            descendants > 10_000_000,
            "descendant counted {descendants} instructions"
        );
        // The leader reaped the subshell, so its record already holds the subshell's time.
        assert!(total.cpu_s >= running.cpu_s, "{total:?} after {running:?}");
    }

    /// The `times` builtin's second line, the CPU seconds of the children a shell reaped, as
    /// `0m1.250s 0m0.031s`.
    fn children_seconds(line: &str) -> f64 {
        line.split_whitespace()
            .map(|field| {
                let (minutes, seconds) = field.trim_end_matches('s').split_once('m').unwrap();
                minutes.parse::<f64>().unwrap() * 60.0 + seconds.parse::<f64>().unwrap()
            })
            .sum()
    }

    /// cargo-nextest starts one process per test, most of which start and exit between two
    /// samples. Their time reaches the leader through the records of the processes that reaped
    /// them, two generations down here.
    #[test]
    fn the_tree_meter_counts_descendants_that_exit_between_samples() {
        let inner = "i=0; while [ $i -lt 40 ]; do                      ( j=0; while [ $j -lt 5000 ]; do j=$((j+1)); done ); i=$((i+1)); done; times";
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &format!("/bin/sh -c '{inner}'; true")])
            .stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        let mut meter = TreeMeter::new(group.id, group.child.id());
        let mut lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let _own = lines.next().unwrap().unwrap();
        let reaped = children_seconds(&lines.next().unwrap().unwrap());
        assert!(reaped > 0.0, "the subshells used no time");

        let (status, leader) = Native.reap(&mut group.child).unwrap();
        assert!(status.success());
        meter.sample().unwrap();
        let total = meter.finish(leader);
        assert!(
            total.cpu_s >= reaped,
            "recorded {} s; the subshells used {reaped} s",
            total.cpu_s
        );
    }

    /// A background process outlives the shell that started it, so launchd, not the leader,
    /// reaps it: only its last reading can count its time.
    #[test]
    fn the_tree_meter_counts_a_descendant_that_lost_its_parent() {
        // A background job's input is /dev/null unless redirected, so it reads the shell's from
        // descriptor 3.
        let script = format!("exec 3<&0; ( {WORK}; echo done; read line <&3 ) & exit 0");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        let mut meter = TreeMeter::new(group.id, group.child.id());
        let mut stdin = group.child.stdin.take().unwrap();
        let mut lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let (status, leader) = Native.reap(&mut group.child).unwrap();
        assert!(status.success());
        assert_eq!(lines.next().unwrap().unwrap(), "done");

        meter.sample().unwrap();
        let worked = Native.tree_cpu_seconds_of(group.id).unwrap();
        assert!(worked > 0.0);
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
        // The background process closes its output when it exits.
        assert!(lines.next().is_none());
        meter.sample().unwrap();

        let total = meter.finish(leader);
        assert!(
            total.cpu_s >= worked,
            "recorded {} s; the background process used {worked} s",
            total.cpu_s
        );
    }

    /// A process that starts a process group of its own escapes `TreeMeter`, which reads only
    /// the command's group, but not the scheduler's reading of the whole tree.
    #[test]
    fn the_tree_cpu_counts_a_descendant_that_left_the_group() {
        // Perl leaves the group, works, prints its PID and waits for its input to close.
        let perl = "setpgrp(0, 0); $| = 1; my $i = 0; $i++ while $i < 3000000; \
                    print \"$$\\n\"; <STDIN>";
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &format!("perl -e '{perl}'; true")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut group = Native.spawn_group(&mut command).unwrap();
        let mut lines = BufReader::new(group.child.stdout.take().unwrap()).lines();
        let escaped: u32 = lines.next().unwrap().unwrap().trim().parse().unwrap();

        assert!(!Native.group_members(group.id).unwrap().contains(&escaped));
        let its_own = read(escaped).unwrap().usage.cpu_s;
        assert!(its_own > 0.0);
        let tree = Native.tree_cpu_seconds_of(group.id).unwrap();
        assert!(
            tree >= its_own,
            "the tree used {tree} s, its descendant {its_own} s"
        );

        drop(group.child.stdin.take());
        let (status, _) = Native.reap(&mut group.child).unwrap();
        assert!(status.success());
    }

    #[test]
    fn the_process_tree_cpu_counts_a_grandchild_in_another_group() {
        // The shell is an ordinary child, in the test's own process group. Perl, its child,
        // leaves for a group of its own, works, prints its PID and waits for its input to close.
        let perl = "setpgrp(0, 0); $| = 1; my $i = 0; $i++ while $i < 3000000; \
                    print \"$$\\n\"; <STDIN>";
        let mut shell = Command::new("/bin/sh")
            .args(["-c", &format!("perl -e '{perl}'; true")])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(shell.stdout.take().unwrap()).lines();
        let grandchild: u32 = lines.next().unwrap().unwrap().trim().parse().unwrap();

        let its_own = read(grandchild).unwrap().usage.cpu_s;
        assert!(its_own > 0.0);
        let tree = Native::process_tree_cpu_seconds_of(shell.id());
        assert!(
            tree >= its_own,
            "the tree used {tree} s, its grandchild {its_own} s"
        );

        drop(shell.stdin.take());
        assert!(shell.wait().unwrap().success());
    }

    /// The scheduler reads the daemon's tree beside the trees of the verb processes it knows,
    /// some of which are the daemon's own children: each process counts once, under the first
    /// root, and a root that has exited counts as nothing.
    #[test]
    fn process_trees_count_each_process_once_and_leave_out_the_dead() {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 20000 ]; do i=$((i+1)); done; read line",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut gone = Command::new("/usr/bin/true").spawn().unwrap();
        let gone_pid = gone.id();
        gone.wait().unwrap();
        let own = std::process::id();
        // Let the shell use some CPU before reading.
        while read(child.id()).unwrap().usage.cpu_s <= 0.0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let readings = Native::process_trees_cpu_seconds_of(&[own, child.id(), gone_pid]);
        let roots: Vec<u32> = readings.iter().map(|&(root, _)| root).collect();
        assert_eq!(roots, [own, child.id()], "the exited root is left out");
        assert!(readings[0].1 > 0.0);
        assert!(
            readings[1].1.abs() < f64::EPSILON,
            "the child was counted in the test's own tree, so it reads 0"
        );

        drop(child.stdin.take());
        child.wait().unwrap();
    }
}
