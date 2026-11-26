//! Splitting a family's run into several commands when one would pass the system's limit on the
//! length of a command line.
//!
//! A `jsonl` family's run command names each check as an argument, and a `nextest` family's
//! names each test in its filter. Windows refuses to start a process whose command line is over
//! 32,767 characters (`ERROR_FILENAME_EXCED_RANGE`, "The filename or extension is too long").
//! macOS enforces a 1 MiB `ARG_MAX` (`1,048,576` bytes) across `argv` strings, `argv` pointers,
//! and the process environment, so large families such as `client-interface` (11,000+ checks
//! totaling nearly 1 MB of names) fail `execve` with `E2BIG` if run in a single invocation. So on
//! every platform the checks are run in as many commands as keep each line under
//! [`MAX_COMMAND_LINE`], one after another, and their reports are joined.

use std::ops::Range;

use ignite_core::ids::RecordId;
use ignite_core::manifest::{Adapter, Family};
use ignite_core::traits::RunResult;

use super::execute::interrupted;
use super::{Checks, Keyed, OpenTree, Reported, RunOptions, adapters};
use crate::server::Context;

/// The longest command line a family's run may have, in bytes: Windows's 32,767-character limit
/// with room for quoting, and on Unix a bounded size well below macOS's 1 MiB `ARG_MAX` to leave
/// headroom for `argv` pointers and environment variables.
const MAX_COMMAND_LINE: usize = if cfg!(windows) { 30_000 } else { 200_000 };

impl Checks {
    /// Runs the family's `run` command for `checks` and reads what it reported, with the ID the
    /// hub gave the run's record if it took it. A run that one command line cannot hold is split
    /// (see the module comment).
    pub(super) fn run_family(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        checks: &[Keyed],
        options: &RunOptions,
    ) -> (Vec<Reported>, RunResult, Option<RecordId>) {
        let fixed = adapters::words(family.run.as_deref().unwrap_or_default())
            .iter()
            .map(|word| word.len() + 1)
            .sum();
        let lengths: Vec<usize> = checks.iter().map(|k| check_length(family, k)).collect();
        let parts = chunks(fixed, &lengths, MAX_COMMAND_LINE);
        let split = parts.len() > 1;
        let mut joined: Option<(Vec<Reported>, RunResult, Option<RecordId>)> = None;
        for part in parts {
            let (reported, result, record) =
                self.run_family_once(ctx, tree, name, family, &checks[part], options);
            let stopped = split && interrupted(&result);
            joined = Some(match joined {
                None => (reported, result, record),
                Some((mut all, before, first)) => {
                    all.extend(reported);
                    (all, join(before, result, stopped), first.or(record))
                }
            });
            if stopped {
                break;
            }
        }
        joined.unwrap_or_else(|| self.run_family_once(ctx, tree, name, family, checks, options))
    }
}

/// The bytes one check adds to its family's run command, including per-argument pointer overhead
/// so Unix `ARG_MAX` accounting includes `argv` pointers.
fn check_length(family: &Family, check: &Keyed) -> usize {
    match (&family.adapter, &check.listed.target) {
        // ` | (binary_id(=<id>) & test(=<test>))` in the filter.
        (Adapter::Nextest, Some(target)) => target.binary_id.len() + target.test.len() + 32,
        // The check's name as an argument, with a space, room for quotes, and an `argv` pointer.
        _ => check.listed.check.as_str().len() + 3 + std::mem::size_of::<usize>(),
    }
}

/// Splits checks of the given lengths into consecutive runs whose command line, `fixed`
/// characters plus their own, stays within `max`. Each run holds at least one check, even one
/// that is too long by itself, which then fails as it would have unsplit.
fn chunks(fixed: usize, lengths: &[usize], max: usize) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut used = fixed;
    for (i, &length) in lengths.iter().enumerate() {
        if i > start && used + length > max {
            parts.push(start..i);
            start = i;
            used = fixed;
        }
        used += length;
    }
    if start < lengths.len() || parts.is_empty() {
        parts.push(start..lengths.len());
    }
    parts
}

/// Two commands of one run as one, with their usage added up. An interrupted command's output
/// takes precedence over the first failure so the caller can detect the cancellation.
fn join(first: RunResult, second: RunResult, stopped: bool) -> RunResult {
    let failed = |r: &RunResult| r.hung || r.exit != Some(0);
    let first_failed = failed(&first);
    let usage = ignite_core::protocol::Usage {
        queued_s: first.usage.queued_s + second.usage.queued_s,
        wall_s: first.usage.wall_s + second.usage.wall_s,
        cpu_s: first.usage.cpu_s + second.usage.cpu_s,
        instructions: first
            .usage
            .instructions
            .zip(second.usage.instructions)
            .map(|(a, b)| a + b),
        peak_mb: first.usage.peak_mb.max(second.usage.peak_mb),
    };
    let hung = !stopped && (first.hung || second.hung);
    if !stopped && (first_failed || !failed(&second)) {
        RunResult {
            usage,
            hung,
            ..first
        }
    } else {
        RunResult {
            usage,
            hung,
            ..second
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_that_fit_one_line_stay_one_run() {
        let one = |checks: usize| std::iter::once(0..checks).collect::<Vec<_>>();
        assert_eq!(chunks(10, &[5, 5, 5], 100), one(3));
        assert_eq!(chunks(10, &[], 100), one(0));
    }

    #[test]
    fn a_long_list_is_split_where_the_next_check_would_pass_the_limit() {
        assert_eq!(chunks(10, &[40, 40, 40, 40, 40], 100), [0..2, 2..4, 4..5]);
    }

    #[test]
    fn a_check_too_long_by_itself_still_runs_alone() {
        assert_eq!(chunks(10, &[5, 200, 5], 100), [0..1, 1..2, 2..3]);
    }

    #[test]
    fn oversized_unix_family_is_split_well_below_arg_max() {
        let fixed = 64;
        // 11,058 checks of ~90 bytes each (plus pointer/separator overhead) would exceed 1 MiB.
        let per_check = 90 + 3 + std::mem::size_of::<usize>();
        let lengths = vec![per_check; 11_058];
        let parts = chunks(fixed, &lengths, MAX_COMMAND_LINE);
        assert!(parts.len() > 1);
        assert_eq!(parts.iter().map(Range::len).sum::<usize>(), 11_058);
        for part in &parts {
            let total = fixed + part.len() * per_check;
            assert!(total <= MAX_COMMAND_LINE);
            assert!(total < 1_048_576);
        }
    }
}
