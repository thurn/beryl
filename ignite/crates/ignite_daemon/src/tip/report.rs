//! What a round tells the hub when it cannot finish a tip by itself: checks that stay
//! incomplete, a build that failed, a family or a check it cannot list or key, and a push to
//! `master` that is refused. Each is told once, and a failure to tell is logged and tried again
//! next round.
//!
//! A report names one cause, never one check: the hub files a ticket per report's cause, so
//! checks held back by one cause go in one report, which lists the first of them and counts
//! all of them. One refused build once filed a P1 ticket for each of its 1,446 checks.

use std::collections::HashMap;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    INCOMPLETE_LISTED, IncompleteReason, IncompleteReport, ResultKey,
};
use ignite_core::verdict;

use super::{TipCheck, TipRun, TipVerifier, Unlisted, tail};
use crate::checks::{LISTING_BUILD_FAILED, UNRESOLVED};
use crate::server::Context;

/// How long this machine leaves a check alone after its first run ended incomplete: an hour.
/// Each later incomplete run doubles it, up to [`GIVE_UP_MAX_S`].
const GIVE_UP_S: f64 = 3_600.0;

/// The longest a check is left alone after incomplete runs: a day.
const GIVE_UP_MAX_S: f64 = 86_400.0;

impl TipVerifier {
    /// Gives up on each check of `verdicts` that stayed incomplete after the attempts of its
    /// run (see [`verdict::MAX_INCOMPLETE`]), which the runner has already made: this machine
    /// leaves it alone for a growing time, and the hub is told once per check, in one report
    /// for each family, reason and platform. A check whose run ended incomplete through no
    /// fault of its own (no landing tree was free) is not given up on; the next round runs it
    /// again. Nor is a check whose build failed: holding it off is for a check that ran and
    /// hung or went unreported, and a build fails the same way at once until its tree or its
    /// inputs change, so holding its checks off only kept `master` waiting for hours after a
    /// tree's damaged build was mended.
    ///
    /// `holds_tip` says whether `tip` waits on these checks before `master` can move; a check
    /// run past the tip budget, after `master` moved, holds nothing back. The hub files a report
    /// that holds the tip released, for the pool to take at once.
    pub(super) fn note_incomplete(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
        holds_tip: bool,
    ) {
        let now = ctx.clock.now();
        self.report_failed_builds(ctx, tip, checks, verdicts, holds_tip);
        // Checks not yet told, by family, reason and platform, with the first one's output.
        let mut causes: Vec<Cause<ResultKey>> = Vec::new();
        for check in checks.iter().filter(|c| c.runnable) {
            let Some(run) = verdicts.get(&check.key) else {
                continue;
            };
            let (Outcome::Incomplete, Some(reason)) = (run.outcome, run.reason) else {
                continue;
            };
            if reason != IncompleteReason::BuildFailed {
                let runs = self.incomplete_runs.entry(check.key.clone()).or_insert(0);
                *runs += 1;
                let wait = (GIVE_UP_S * f64::from(1_u32 << (*runs - 1).min(10))).min(GIVE_UP_MAX_S);
                self.held_off
                    .insert(check.key.clone(), now.plus_seconds(wait));
            }
            if self.reported_incomplete.contains(&check.key) {
                continue;
            }
            let group = (check.family.clone(), reason, check.key.platform);
            let at = if let Some(at) = causes.iter().position(|c| c.group == group) {
                at
            } else {
                let output = run
                    .output
                    .as_deref()
                    .map(|o| tail(o, &self.own_outputs))
                    .unwrap_or_default();
                causes.push(Cause::new(group, output));
                causes.len() - 1
            };
            causes[at].add(check.key.clone(), &check.check);
        }
        let attempts = u32::try_from(verdict::MAX_INCOMPLETE).unwrap_or(u32::MAX);
        for cause in causes {
            let (_, reason, platform) = cause.group.clone();
            let mut report =
                incomplete_report(ctx, tip, &cause.checks[0], platform, reason, attempts, "");
            cause.fill(&mut report);
            report.holds_tip = holds_tip;
            self.name_host(&mut report);
            match ctx.hub.incomplete(&report) {
                Ok(_) => self.reported_incomplete.extend(cause.keys),
                Err(error) => {
                    eprintln!("ignite: incomplete {} not reported: {error}", report.check);
                }
            }
        }
    }

    /// Tells the hub once per tip of each product whose failed build kept checks from running,
    /// naming the first family's `<family>:build` and quoting the build's output, with the
    /// number of checks that wait on it. The checks are not held off, since the build is not
    /// run again until an input changes.
    fn report_failed_builds(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
        holds_tip: bool,
    ) {
        // Each failed product, with the name of its report: the first family's `<family>:build`.
        let mut builds: Vec<(CheckName, Cause<()>)> = Vec::new();
        for check in checks.iter().filter(|c| c.runnable) {
            let Some(run) = verdicts.get(&check.key) else {
                continue;
            };
            let (Outcome::Incomplete, Some(product)) = (run.outcome, &run.build) else {
                continue;
            };
            let group = (product.clone(), IncompleteReason::BuildFailed, None);
            let at = if let Some(at) = builds.iter().position(|(_, c)| c.group == group) {
                at
            } else {
                let Ok(name) = CheckName::new(format!("{}:build", check.family)) else {
                    continue;
                };
                let output = run.output.clone().unwrap_or_default();
                let text = format!(
                    "The build of {product} failed, so no check of {} ran.\n\n{}",
                    check.family,
                    tail(&output, &self.own_outputs)
                );
                builds.push((name, Cause::new(group, text)));
                builds.len() - 1
            };
            builds[at].1.add((), &check.check);
        }
        for (name, build) in builds {
            let mut report =
                incomplete_report(ctx, tip, &name, None, IncompleteReason::BuildFailed, 1, "");
            build.fill(&mut report);
            report.product = Some(build.group.0);
            report.holds_tip = holds_tip;
            self.send_once(ctx, tip, &report);
        }
    }

    /// Tells the hub once per tip of each family or check in `unlisted` that this machine runs
    /// itself: checks whose inputs cannot be resolved in one report per family and message,
    /// a listing whose build failed in one report per product, and any other family that
    /// cannot be listed as an incomplete-check report with the reason `Unreported`. The output
    /// says what was wrong.
    pub(super) fn report_unlisted(&mut self, ctx: &Context, tip: &Commit, unlisted: &[Unlisted]) {
        let mut entries: Vec<Cause<CheckName>> = Vec::new();
        for missing in unlisted.iter().filter(|u| u.reportable) {
            if let Some((_, message)) = missing.message.split_once(UNRESOLVED) {
                let group = (
                    format!("{}\n{message}", missing.family),
                    IncompleteReason::InvalidEntry,
                    None,
                );
                let at = if let Some(at) = entries.iter().position(|c| c.group == group) {
                    at
                } else {
                    entries.push(Cause::new(group, message.to_owned()));
                    entries.len() - 1
                };
                entries[at].add(missing.name.clone(), &missing.name);
            } else if let Some((_, product)) = missing.message.split_once(LISTING_BUILD_FAILED) {
                let mut report = incomplete_report(
                    ctx,
                    tip,
                    &missing.name,
                    None,
                    IncompleteReason::BuildFailed,
                    1,
                    &format!(
                        "The build of {product} failed, so {} could not be listed.",
                        missing.family
                    ),
                );
                report.product = Some(product.to_owned());
                self.send_once(ctx, tip, &report);
            } else {
                self.report_stuck(ctx, tip, &missing.name, None, &missing.message);
            }
        }
        for entry in entries {
            let mut report = incomplete_report(
                ctx,
                tip,
                &entry.checks[0],
                None,
                IncompleteReason::InvalidEntry,
                1,
                "",
            );
            entry.fill(&mut report);
            self.send_once(ctx, tip, &report);
        }
    }

    /// Tells the hub once per tip and name that the tip is stuck on `name`, with `output`.
    pub(super) fn report_stuck(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        name: &CheckName,
        platform: Option<Os>,
        output: &str,
    ) {
        let report = incomplete_report(
            ctx,
            tip,
            name,
            platform,
            IncompleteReason::Unreported,
            1,
            output,
        );
        self.send_once(ctx, tip, &report);
    }

    /// Sends `report` unless this tip already told the hub about its check and the checks it
    /// lists, and remembers all of them once the hub took it.
    fn send_once(&mut self, ctx: &Context, tip: &Commit, report: &IncompleteReport) {
        let mut names: Vec<CheckName> = vec![report.check.clone()];
        names.extend(report.checks.iter().cloned());
        let keys: Vec<(Commit, CheckName)> =
            names.into_iter().map(|name| (tip.clone(), name)).collect();
        if keys.iter().all(|key| self.reported_stuck.contains(key)) {
            return;
        }
        let mut report = report.clone();
        self.name_host(&mut report);
        match ctx.hub.incomplete(&report) {
            Ok(_) => self.reported_stuck.extend(keys),
            Err(error) => eprintln!("ignite: {} not reported: {error}", report.check),
        }
    }

    /// Puts this machine's hostname in a report's `host`, when the verifier knows it.
    fn name_host(&self, report: &mut IncompleteReport) {
        if !self.host.is_empty() {
            report.host.clone_from(&self.host);
        }
    }
}

/// The checks one cause holds back, gathered for one report: what tells causes apart, its
/// output, and every check (with a key the verifier remembers it by).
struct Cause<K> {
    group: (String, IncompleteReason, Option<Os>),
    output: String,
    checks: Vec<CheckName>,
    keys: Vec<K>,
}

impl<K> Cause<K> {
    fn new(group: (String, IncompleteReason, Option<Os>), output: String) -> Self {
        Cause {
            group,
            output,
            checks: Vec::new(),
            keys: Vec::new(),
        }
    }

    fn add(&mut self, key: K, check: &CheckName) {
        self.keys.push(key);
        self.checks.push(check.clone());
    }

    /// Puts the cause's output, its first [`INCOMPLETE_LISTED`] checks and its count in
    /// `report`.
    fn fill(&self, report: &mut IncompleteReport) {
        report.output.clone_from(&self.output);
        report.checks = self
            .checks
            .iter()
            .take(INCOMPLETE_LISTED)
            .cloned()
            .collect();
        report.count = u32::try_from(self.checks.len()).unwrap_or(u32::MAX);
    }
}

/// The report of `check`, naming the machine by its ID in both `machine` and `host`; the caller
/// puts the hostname in `host` when it knows it (see `TipVerifier::name_host`). The report says
/// it holds the tip back; a check run after `master` moved is reported with `holds_tip` unset.
fn incomplete_report(
    ctx: &Context,
    tip: &Commit,
    check: &CheckName,
    platform: Option<Os>,
    reason: IncompleteReason,
    attempts: u32,
    output: &str,
) -> IncompleteReport {
    IncompleteReport {
        check: check.clone(),
        machine: ctx.machine.clone(),
        host: ctx.machine.to_string(),
        platform,
        attempts,
        reason,
        commit: tip.clone(),
        output: output.to_owned(),
        checks: Vec::new(),
        count: 0,
        product: None,
        holds_tip: true,
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
