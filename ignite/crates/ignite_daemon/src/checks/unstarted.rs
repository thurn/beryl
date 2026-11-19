//! Runs whose test processes probably never started.
//!
//! When every check in a run fails, or a command reports nothing at all, the usual cause is the
//! machine rather than the checks: the test runner's executable is missing, the user's process
//! limit is reached, or memory or disk ran out. Once the run is over, nothing shows which of
//! these it was. So at that moment [`record`] appends a description of the machine to the run's
//! output file and prints it to the daemon log. The hub quotes the end of that file in the
//! ticket it files when many checks flake at once, so the description reaches that ticket too.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

use ignite_core::protocol::Outcome;
use ignite_core::traits::RunResult;

use super::Reported;
use crate::doctor::{Host, NativeHost, command_output};

/// A run in which at least this many checks were reported, all of them failures, probably
/// never started its test processes. It matches the number of checks flaking in one command
/// that makes the hub blame the machine.
pub(super) const ALL_FAILED: usize = 3;

/// At most this many lines that repeat in every failure are quoted.
const REPEATED_LINES: usize = 3;

/// Whether the run of `pending` checks that reported `reported` and ended as `result` looks as
/// if its checks never started: at least [`ALL_FAILED`] checks reported and every one failed,
/// or nothing reported although the command neither passed nor hung.
pub(super) fn looks_unstarted(reported: &[Reported], result: &RunResult, pending: usize) -> bool {
    let failed = reported
        .iter()
        .filter(|r| r.outcome == Outcome::Fail)
        .count();
    let all_failed = failed >= ALL_FAILED && failed == reported.len();
    let silent = pending > 0 && reported.is_empty() && !result.hung && result.exit != Some(0);
    all_failed || silent
}

/// What the machine looked like when a run's checks failed to start. Each field is `None` when
/// it could not be read, as on an operating system without the tool that reads it.
#[derive(Debug, Default)]
pub(super) struct Machine {
    /// Files this process has open.
    pub open_files: Option<usize>,
    /// `ulimit -n`: the most files one process may have open.
    pub open_files_limit: Option<String>,
    /// Processes of this user, and how many of them are zombies (exited but never waited for).
    pub processes: Option<(usize, usize)>,
    /// `ulimit -u`: the most processes this user may have.
    pub process_limit: Option<String>,
    /// `kern.maxprocperuid`: the system's own limit of processes per user, on macOS.
    pub max_per_user: Option<String>,
    /// Free memory in megabytes, from `vm_stat` on macOS.
    pub free_memory_mb: Option<u64>,
    /// `vm.swapusage` on macOS, such as `total = 2048.00M  used = 1024.00M  free = 1024.00M`.
    pub swap: Option<String>,
    /// Gigabytes free on the disk that holds the build tree.
    pub disk_free_gb: Option<u64>,
}

impl Machine {
    /// Reads the machine now, for a run in the build tree at `tree`.
    pub fn read(tree: &Path) -> Machine {
        let shell = |what: &str| command_output("sh", &["-c", what]);
        let processes = command_output("ps", &["-x", "-o", "stat="]).map(|text| {
            let states: Vec<&str> = text.lines().map(str::trim).collect();
            let zombies = states.iter().filter(|s| s.starts_with('Z')).count();
            (states.len(), zombies)
        });
        Machine {
            open_files: std::fs::read_dir("/dev/fd").ok().map(Iterator::count),
            open_files_limit: shell("ulimit -n"),
            processes,
            process_limit: shell("ulimit -u"),
            max_per_user: command_output("sysctl", &["-n", "kern.maxprocperuid"]),
            free_memory_mb: command_output("vm_stat", &[]).and_then(|text| free_mb(&text)),
            swap: command_output("sysctl", &["-n", "vm.swapusage"]),
            disk_free_gb: NativeHost.disk_free_gb(tree),
        }
    }

    /// The description's lines, each naming what it measured.
    fn lines(&self) -> Vec<String> {
        let unknown = || "unknown".to_owned();
        let shown = |value: Option<String>| value.unwrap_or_else(unknown);
        vec![
            format!(
                "open files of the daemon: {} (limit per process {})",
                shown(self.open_files.map(|n| n.to_string())),
                shown(self.open_files_limit.clone())
            ),
            format!(
                "processes of this user: {} (limit `ulimit -u` {}, kern.maxprocperuid {})",
                shown(
                    self.processes
                        .map(|(all, zombies)| format!("{all}, {zombies} of them zombies"))
                ),
                shown(self.process_limit.clone()),
                shown(self.max_per_user.clone())
            ),
            format!(
                "free memory: {}; swap: {}",
                shown(self.free_memory_mb.map(|mb| format!("{mb} MB"))),
                shown(self.swap.clone())
            ),
            format!(
                "free disk where the tree is: {}",
                shown(self.disk_free_gb.map(|gb| format!("{gb} GB")))
            ),
        ]
    }
}

/// Free memory in megabytes from `vm_stat`'s output: its free and speculative pages, which
/// the system can hand out at once, times the page size its first line states.
fn free_mb(vm_stat: &str) -> Option<u64> {
    let page: u64 = vm_stat
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |label: &str| -> Option<u64> {
        let line = vm_stat.lines().find(|l| l.starts_with(label))?;
        line.split(':')
            .nth(1)?
            .trim()
            .trim_end_matches('.')
            .parse()
            .ok()
    };
    let free = pages("Pages free")? + pages("Pages speculative").unwrap_or(0);
    Some(free * page / (1024 * 1024))
}

/// The lines of `output` that appear at least once for each of `failed` failures, most
/// frequent first: when every test fails the same way, such as `exec: ... No such file or
/// directory` from a missing runner, that line names the cause. Machine-readable events
/// (lines that start with `{`) and lines made only of decoration are left out.
fn repeated_lines(output: &str, failed: usize) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for line in output.lines().map(str::trim) {
        if line.starts_with('{') || !line.chars().any(char::is_alphanumeric) {
            continue;
        }
        *counts.entry(line).or_default() += 1;
    }
    let mut repeated: Vec<(&str, usize)> = counts
        .into_iter()
        .filter(|&(_, count)| count >= failed.max(2))
        .collect();
    repeated.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    repeated
        .into_iter()
        .take(REPEATED_LINES)
        .map(|(line, count)| format!("{count}x {line}"))
        .collect()
}

/// The description appended to a run's output: why it was written, the first failure's
/// message, the lines every failure repeated, and `machine`.
pub(super) fn describe(reported: &[Reported], output: &str, machine: &Machine) -> String {
    let failed: Vec<&Reported> = reported
        .iter()
        .filter(|r| r.outcome == Outcome::Fail)
        .collect();
    let mut text = if failed.is_empty() {
        "──── ignite: the command reported no check, so it may not have started. The machine \
         when it ended:\n"
            .to_owned()
    } else {
        format!(
            "──── ignite: all {} checks in this run failed, so their processes may not have \
             started. The machine when the run ended:\n",
            failed.len()
        )
    };
    if let Some(first) = failed.first() {
        let said = [first.at.as_deref(), first.message.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(": ");
        let said = if said.is_empty() {
            "(no message)".to_owned()
        } else {
            said.lines().take(5).collect::<Vec<_>>().join(" / ")
        };
        let _ = writeln!(text, "first failure: {}: {said}", first.check);
    }
    for line in repeated_lines(output, failed.len()) {
        let _ = writeln!(text, "repeated in every failure: {line}");
    }
    for line in machine.lines() {
        let _ = writeln!(text, "{line}");
    }
    text
}

/// Appends the description of `machine` to the run's output file and prints it to the daemon
/// log.
pub(super) fn record(result: &RunResult, reported: &[Reported], machine: &Machine) {
    let output = std::fs::read_to_string(&result.output).unwrap_or_default();
    let text = describe(reported, &output, machine);
    eprintln!(
        "ignite: a run's checks may not have started ({}):\n{text}",
        result.output.display()
    );
    let appended = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&result.output)
        .and_then(|mut file| file.write_all(format!("\n{text}").as_bytes()));
    if let Err(error) = appended {
        eprintln!(
            "ignite: cannot add the machine's state to {}: {error}",
            result.output.display()
        );
    }
}

#[cfg(test)]
#[path = "unstarted_tests.rs"]
mod tests;
