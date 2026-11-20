//! A gate that fails on checks which already fail at the tip the landing was rebased onto
//! (see "Landing" in `docs/ignite.md`).
//!
//! On 2026-11-19 a landing that changed one check manifest was refused for about 28 findings of
//! `source:check-inputs` that an earlier landing had introduced. The gate left a check out as
//! known to fail only once tip verification had filed its regression ticket, so between a fault
//! landing and that ticket, every landing whose change touched the check's inputs failed on it,
//! and the session rightly would not fix another ticket's fault.
//!
//! Now, when every fault of a gate is a check failure that its rerun confirmed, the landing
//! asks whether each of those checks also fails at the tip it was rebased onto. It runs them in
//! a landing tree checked out at that tip, each from the result cache where that tip's input
//! hash has a result. Each check that fails there too is filed with the hub as a regression at
//! once, as tip verification would file it. When every failure fails at the tip too, the gate
//! runs again with those checks left out as known to fail, and the checks that already passed
//! come from the cache. A check that passes at the tip refuses the landing as before.
//!
//! A check that fails at the tip counts as known to fail even when its failure in the landing
//! says something else, such as more findings: the adapters report no list of findings that
//! could be compared.

use ignite_core::ids::{CheckName, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::tipround::CulpritSearch;

use super::env::{CheckVerdict, GateTree};
use super::passes::Passed;
use super::{Landing, Rebased, short};
use crate::checks::{Carried, Excused, GateRun, RunOptions};
use crate::server::Context;
use crate::steps;

/// What one pass of a landing gates, and where.
pub(super) struct Pass<'a> {
    /// The name the landing leases landing trees under.
    pub holder: &'a str,
    pub tree: &'a GateTree,
    /// `tree` is a landing tree this landing holds, which may be checked out at the tip for a
    /// moment when no other landing tree is free. A ticket tree may not: it holds the branch.
    pub borrowable: bool,
    pub rebased: &'a Rebased,
    pub options: &'a RunOptions,
    pub carried: Option<&'a Carried>,
    /// The last gate of this landing that passed, whose verdicts `carried` carries over.
    pub earlier: Option<&'a Passed>,
    /// `rebased.onto` is the tip, so a check failing there too is filed as a regression and
    /// left out. A landing by verdict gates on its base, which may be older than the tip and
    /// even than `master`: a check failing there may be fixed at the tip, so it is not filed,
    /// and the landing is refused with the advice to rebase.
    pub onto_tip: bool,
}

impl Landing {
    /// Runs the gate, leaving out the checks in `excused` as known to fail. When every fault is
    /// a confirmed failure of a check that fails at the tip too, adds those checks to
    /// `excused` and runs the gate again. `excused` lasts for the whole landing, so a later
    /// pass does not ask again.
    pub(super) fn gate_pass(
        &self,
        ctx: &Context,
        pass: &Pass<'_>,
        excused: &mut Excused,
    ) -> GateRun {
        loop {
            let gated = self
                .gate
                .run(ctx, pass.tree, pass.options, pass.carried, excused);
            let mut gated = match pass.earlier {
                Some(earlier) if pass.carried.is_some() => earlier.merge(gated),
                _ => gated,
            };
            // A check left out as known to fail still runs when the change edits its own
            // file, and then its failure is the change's.
            let Some(failed) = confirmed_failures(&gated)
                .filter(|failed| !failed.iter().any(|c| excused.contains_key(c)))
            else {
                return gated;
            };
            if steps::stop_requested() {
                return gated;
            }
            let verdicts = match self.at_base(ctx, pass, &failed) {
                Ok(verdicts) => verdicts,
                Err(why) => {
                    gated.base = Some(format!("not compared with the tip: {why}"));
                    return gated;
                }
            };
            let mut passing = Vec::new();
            if !pass.onto_tip {
                let at_base: Vec<String> = verdicts
                    .iter()
                    .filter(|v| v.outcome == Outcome::Fail && failed.contains(&v.check))
                    .map(|v| v.check.to_string())
                    .collect();
                if at_base.len() == failed.len() {
                    gated.base = Some(format!(
                        "not compared with the tip: {} fails at the base {} as well, which may \
                         be older than the tip; `ignite sync` rebases onto the tip, then land again",
                        at_base.join(", "),
                        short(&pass.rebased.onto)
                    ));
                    gated.fails_at_older_base = true;
                    return gated;
                }
            }
            for check in &failed {
                let verdict = verdicts.iter().find(|v| v.check == *check);
                match verdict.filter(|v| v.outcome == Outcome::Fail) {
                    Some(_) if !pass.onto_tip => {
                        passing.push(format!("{check} (it fails at the base too)"));
                    }
                    Some(verdict) => match file(ctx, pass.rebased, verdict) {
                        Some(ticket) => {
                            excused.insert(check.clone(), ticket);
                        }
                        None => passing.push(format!("{check} (its regression was not filed)")),
                    },
                    None => passing.push(check.to_string()),
                }
            }
            if !passing.is_empty() {
                gated.base = Some(format!(
                    "{} passes at {}, so the failure is this change's",
                    passing.join(", "),
                    short(&pass.rebased.onto)
                ));
                return gated;
            }
        }
    }

    /// Runs `checks` at the tip the landing was rebased onto: in a free landing tree checked
    /// out there, or else in the landing's own landing tree, which is then checked out at the
    /// rebased commit again. `Err` says why neither could be done.
    fn at_base(
        &self,
        ctx: &Context,
        pass: &Pass<'_>,
        checks: &[CheckName],
    ) -> Result<Vec<CheckVerdict>, String> {
        let onto = &pass.rebased.onto;
        let n = checks.len();
        let word = if n == 1 { "check" } else { "checks" };
        steps::step(format!(
            "running {n} failing {word} at the tip {}",
            short(onto)
        ));
        let holder = format!("{}:base", pass.holder);
        let free = self.trees.lease_landing(&holder, onto);
        if let Ok(Some(tree)) = free {
            let verdicts = self.gate.run_checks(ctx, &tree, checks, pass.options);
            if let Err(error) = self.trees.release_landing(&tree.tree, Some(onto)) {
                eprintln!("ignite: landing tree {} not freed: {error}", tree.tree);
            }
            return Ok(verdicts);
        }
        if !pass.borrowable {
            return Err("no landing tree is free".to_owned());
        }
        // The landing's own tree, asked for again under the landing's name, is checked out at
        // the tip and then back at the rebased commit, which the gate runs on again.
        let moved = self.trees.lease_landing(pass.holder, onto);
        if !matches!(&moved, Ok(Some(tree)) if tree.tree == pass.tree.tree) {
            self.trees
                .lease_landing(pass.holder, &pass.rebased.head)
                .map_err(|e| e.to_string())?;
            return Err(format!(
                "{} could not be checked out at the tip",
                pass.tree.tree
            ));
        }
        let verdicts = self.gate.run_checks(ctx, pass.tree, checks, pass.options);
        match self.trees.lease_landing(pass.holder, &pass.rebased.head) {
            Ok(Some(tree)) if tree.tree == pass.tree.tree => Ok(verdicts),
            _ => Err(format!(
                "{} could not be checked out at the rebased commit again",
                pass.tree.tree
            )),
        }
    }
}

/// The checks whose failures the gate's rerun confirmed, when they are its only faults; `None`
/// for a gate that passed or that a build, a listing or an incomplete check refused.
fn confirmed_failures(gated: &GateRun) -> Option<Vec<CheckName>> {
    let run = &gated.run;
    let only_failures = run.build_failures.is_empty()
        && run.incomplete_families.is_empty()
        && !run.failures.is_empty()
        && run.failures.iter().all(|f| f.outcome == Outcome::Fail);
    (!run.passed() && only_failures).then(|| run.failures.iter().map(|f| f.check.clone()).collect())
}

/// Files `verdict`'s check with the hub as a regression at the tip the landing was rebased
/// onto, as tip verification would, and returns the ticket the hub filed it on or added it to.
/// No culprit search runs: the report names the commits since the last verified one.
fn file(ctx: &Context, rebased: &Rebased, verdict: &CheckVerdict) -> Option<TicketId> {
    let repository = &ctx.config.repository;
    let last_good = ctx
        .git
        .tip(&repository.master)
        .unwrap_or_else(|_| rebased.onto.clone());
    let report = RegressionReport {
        check: verdict.check.clone(),
        first_bad: rebased.onto.clone(),
        last_good,
        caused_by: None,
        output: verdict.output.clone(),
        suspects: Vec::new(),
        platform: verdict.platform,
        search: Some(CulpritSearch::NotRun {
            why: format!(
                "a landing's gate found the check failing at the {} tip it was rebased onto, \
                 before tip verification reached that tip",
                repository.staging
            ),
        }),
        found_by: FoundBy::Landing,
    };
    match ctx.hub.regression(&report) {
        Ok(filed) => Some(filed.ticket),
        Err(error) => {
            eprintln!(
                "ignite: the regression of {} was not filed: {error}",
                verdict.check
            );
            None
        }
    }
}
