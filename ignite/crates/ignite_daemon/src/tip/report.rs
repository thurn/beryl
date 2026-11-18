//! What a round tells the hub when it cannot finish a tip by itself: a check that stays
//! incomplete, a family or a check it cannot list or key, and a push to `master` that is
//! refused. Each is told once, and a failure to tell is logged and tried again next round.

use std::collections::HashMap;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{IncompleteReason, IncompleteReport, ResultKey};
use ignite_core::verdict;

use super::{TipCheck, TipRun, TipVerifier, Unlisted, tail};
use crate::server::Context;

/// How long this machine leaves a check alone after its first run ended incomplete: an hour.
/// Each later incomplete run doubles it, up to [`GIVE_UP_MAX_S`].
const GIVE_UP_S: f64 = 3_600.0;

/// The longest a check is left alone after incomplete runs: a day.
const GIVE_UP_MAX_S: f64 = 86_400.0;

impl TipVerifier {
    /// Gives up on each check of `verdicts` that stayed incomplete after the attempts of its
    /// run (see [`verdict::MAX_INCOMPLETE`]), which the runner has already made: this machine
    /// leaves it alone for a growing time, and the hub files a ticket once per check and
    /// inputs. A check whose run ended incomplete through no fault of its own (no landing tree
    /// was free) is not given up on; the next round runs it again.
    pub(super) fn note_incomplete(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
    ) {
        let now = ctx.clock.now();
        self.report_failed_builds(ctx, tip, checks, verdicts);
        for check in checks.iter().filter(|c| c.runnable) {
            let Some(run) = verdicts.get(&check.key) else {
                continue;
            };
            let (Outcome::Incomplete, Some(reason)) = (run.outcome, run.reason) else {
                continue;
            };
            let runs = self.incomplete_runs.entry(check.key.clone()).or_insert(0);
            *runs += 1;
            let wait = (GIVE_UP_S * f64::from(1_u32 << (*runs - 1).min(10))).min(GIVE_UP_MAX_S);
            self.held_off
                .insert(check.key.clone(), now.plus_seconds(wait));
            if self.reported_incomplete.contains(&check.key) {
                continue;
            }
            let output = run
                .output
                .as_deref()
                .map(|o| tail(o, &self.own_outputs))
                .unwrap_or_default();
            let mut report = incomplete_report(
                ctx,
                tip,
                &check.check,
                check.key.platform,
                reason,
                u32::try_from(verdict::MAX_INCOMPLETE).unwrap_or(u32::MAX),
                output,
            );
            self.name_host(&mut report);
            match ctx.hub.incomplete(&report) {
                Ok(_) => {
                    self.reported_incomplete.insert(check.key.clone());
                }
                Err(error) => eprintln!("ignite: incomplete {} not reported: {error}", check.check),
            }
        }
    }

    /// Tells the hub once per tip of each family whose checks a failed build kept from running,
    /// naming the check `<family>:build` and quoting the build's output. One build used to file
    /// a ticket for each of its checks: 1,456 P1 tickets for one refused Rust build. The checks
    /// are not held off, since the build is not run again until an input changes.
    fn report_failed_builds(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
    ) {
        let mut families: Vec<(String, String, String)> = Vec::new();
        for check in checks.iter().filter(|c| c.runnable) {
            let Some(run) = verdicts.get(&check.key) else {
                continue;
            };
            let (Outcome::Incomplete, Some(product)) = (run.outcome, &run.build) else {
                continue;
            };
            if !families
                .iter()
                .any(|(family, _, _)| *family == check.family)
            {
                let output = run.output.clone().unwrap_or_default();
                families.push((check.family.clone(), product.clone(), output));
            }
        }
        for (family, product, output) in families {
            let Ok(name) = CheckName::new(format!("{family}:build")) else {
                continue;
            };
            let text = format!(
                "The build of {product} failed, so no check of {family} ran.\n\n{}",
                tail(&output, &self.own_outputs)
            );
            self.report_stuck(ctx, tip, &name, None, &text);
        }
    }

    /// Tells the hub once per tip of each family or check in `unlisted` that this machine runs
    /// itself. The report is an incomplete-check report with the reason `Unreported`, which
    /// the hub files as a P1 ticket naming the machine; the output says what was wrong.
    pub(super) fn report_unlisted(&mut self, ctx: &Context, tip: &Commit, unlisted: &[Unlisted]) {
        for missing in unlisted.iter().filter(|u| u.reportable) {
            self.report_stuck(ctx, tip, &missing.name, None, &missing.message);
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
        let key = (tip.clone(), name.clone());
        if self.reported_stuck.contains(&key) {
            return;
        }
        let mut report = incomplete_report(
            ctx,
            tip,
            name,
            platform,
            IncompleteReason::Unreported,
            1,
            output.to_owned(),
        );
        self.name_host(&mut report);
        match ctx.hub.incomplete(&report) {
            Ok(_) => {
                self.reported_stuck.insert(key);
            }
            Err(error) => eprintln!("ignite: {name} not reported: {error}"),
        }
    }

    /// Puts this machine's hostname in a report's `host`, when the verifier knows it.
    fn name_host(&self, report: &mut IncompleteReport) {
        if !self.host.is_empty() {
            report.host.clone_from(&self.host);
        }
    }
}

/// The report of `check`, naming the machine by its ID in both `machine` and `host`; the caller
/// puts the hostname in `host` when it knows it (see `TipVerifier::name_host`).
fn incomplete_report(
    ctx: &Context,
    tip: &Commit,
    check: &CheckName,
    platform: Option<Os>,
    reason: IncompleteReason,
    attempts: u32,
    output: String,
) -> IncompleteReport {
    IncompleteReport {
        check: check.clone(),
        machine: ctx.machine.clone(),
        host: ctx.machine.to_string(),
        platform,
        attempts,
        reason,
        commit: tip.clone(),
        output,
    }
}
