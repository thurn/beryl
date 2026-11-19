//! Splitting a family's run into several commands when one would pass Windows its limit on the
//! length of a command line.
//!
//! A `jsonl` family's run command names each check as an argument, and a `nextest` family's
//! names each test in its filter. Windows refuses to start a process whose command line is over
//! 32,767 characters (`ERROR_FILENAME_EXCED_RANGE`, "The filename or extension is too long").
//! One Python test family measured on 2026-11-19 lists 1,021 checks in 1.7 MB of names, and a
//! tip claim of Rust tests holds hundreds. So on Windows the checks are run in as many commands as keep each line under
//! [`MAX_COMMAND_LINE`], one after another, and their reports are joined. macOS allows about a
//! megabyte, so there every run is one command, as before.

use std::ops::Range;

use ignite_core::ids::RecordId;
use ignite_core::manifest::{Adapter, Family};
use ignite_core::traits::RunResult;

use super::{Checks, Keyed, OpenTree, Reported, RunOptions, adapters};
use crate::server::Context;

/// The longest command line a family's run may have, in characters: Windows's limit with room
/// for quoting. `None` where the system allows far more than a run ever needs.
const MAX_COMMAND_LINE: Option<usize> = if cfg!(windows) { Some(30_000) } else { None };

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
        let parts = MAX_COMMAND_LINE.map_or_else(
            || std::iter::once(0..checks.len()).collect(),
            |max| {
                let fixed = adapters::words(family.run.as_deref().unwrap_or_default())
                    .iter()
                    .map(|word| word.len() + 1)
                    .sum();
                let lengths: Vec<usize> = checks.iter().map(|k| check_length(family, k)).collect();
                chunks(fixed, &lengths, max)
            },
        );
        let mut joined: Option<(Vec<Reported>, RunResult, Option<RecordId>)> = None;
        for part in parts {
            let (reported, result, record) =
                self.run_family_once(ctx, tree, name, family, &checks[part], options);
            joined = Some(match joined {
                None => (reported, result, record),
                Some((mut all, before, first)) => {
                    all.extend(reported);
                    (all, join(before, result), first.or(record))
                }
            });
        }
        joined.unwrap_or_else(|| self.run_family_once(ctx, tree, name, family, checks, options))
    }
}

/// The characters one check adds to its family's run command.
fn check_length(family: &Family, check: &Keyed) -> usize {
    match (&family.adapter, &check.listed.target) {
        // ` | (binary_id(=<id>) & test(=<test>))` in the filter.
        (Adapter::Nextest, Some(target)) => target.binary_id.len() + target.test.len() + 32,
        // The check's name as an argument, with a space and room for quotes.
        _ => check.listed.check.as_str().len() + 3,
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

/// Two consecutive commands of one run as one: the first failing exit code, the output of the
/// first command that failed, and their usage added up.
fn join(first: RunResult, second: RunResult) -> RunResult {
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
    let hung = first.hung || second.hung;
    if first_failed || !failed(&second) {
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
}
