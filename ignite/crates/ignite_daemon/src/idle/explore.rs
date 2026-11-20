//! Exploration: checks nobody has played, from each exploration family's `next` command, run in
//! a landing tree at the verified commit. A failure becomes a report to the hub, which files a
//! P2 bug labelled `found-by:<family>` or comments on the open one with the same signature.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use ignite_core::ids::{CheckName, Commit, TreeId};
use ignite_core::manifest::{Adapter, Family};
use ignite_core::protocol::hub::ExplorationFind;
use ignite_core::protocol::{Class, Outcome, Usage};
use ignite_core::traits::{BuildResult, CommandSpec, Error, Result};

use super::state::{NewCheck, Report};
use super::{Idle, IdleJob, build_failure};
use crate::checks::adapters::{self, Reported};
use crate::server::Context;

/// The days of spending that decide which family runs next.
const WINDOW_DAYS: f64 = 30.0;

/// The most lines of a run's output a report carries.
const OUTPUT_LINES: usize = 40;

impl Idle {
    /// The exploration family to run next: of those not waiting after a failure, the one that
    /// has spent least in the last 30 days for its share of idle time.
    pub(super) fn next_family(&self) -> Option<String> {
        let families = self.exploration_families();
        #[allow(clippy::cast_precision_loss)]
        let equal = 1.0 / families.len().max(1) as f64;
        let ready: Vec<String> = families
            .into_iter()
            .filter(|family| {
                self.ready(&IdleJob::Explore {
                    family: family.clone(),
                })
            })
            .collect();
        let state = self.state();
        let shares = super::lock(&self.shares).clone();
        let cutoff = self.cutoff();
        let need = |family: &String| {
            let share = shares
                .iter()
                .find(|(f, _)| f == family)
                .map_or(equal, |(_, share)| *share)
                .max(1e-9);
            let spent: f64 = state.spent.get(family).map_or(0.0, |days| {
                days.range(cutoff.clone()..).map(|(_, spent)| spent).sum()
            });
            spent / share
        };
        ready
            .iter()
            .min_by(|a, b| need(a).total_cmp(&need(b)))
            .cloned()
    }

    /// Runs one batch of `family`'s new checks. False if no landing tree was free, so nothing
    /// ran.
    pub(super) fn explore(&self, ctx: &Context, family: &str) -> Result<bool> {
        let manifest = self.settings.manifest.get();
        let declared = manifest
            .families
            .get(family)
            .ok_or_else(|| Error::NotFound(format!("family {family}")))?
            .family
            .clone();
        if declared.adapter != Adapter::Jsonl {
            return Err(Error::Invalid(format!(
                "the exploration family {family} must use the jsonl adapter"
            )));
        }
        let commit = Self::verified(ctx)?;
        let Some((tree, path)) = self.host.lease(&commit)? else {
            return Ok(false);
        };
        let result = self.explore_in(ctx, family, &declared, &tree, &path, &commit);
        let built = matches!(result, Ok(true));
        let released = self.host.release(&tree, built.then_some(&commit));
        result?;
        released?;
        self.send_reports(ctx);
        Ok(true)
    }

    /// Runs the family's saved batch, or a new one from `next`, in `tree`. True if the family's
    /// build products were made current there.
    fn explore_in(
        &self,
        ctx: &Context,
        name: &str,
        family: &Family,
        tree: &TreeId,
        path: &Path,
        commit: &Commit,
    ) -> Result<bool> {
        let saved = self.state().batches.get(name).cloned();
        let batch = if let Some(batch) = saved {
            batch
        } else {
            let batch = self.ask_next(ctx, name, family, tree, path)?;
            if batch.is_empty() {
                return Ok(false);
            }
            self.update(|state| state.batches.insert(name.to_owned(), batch.clone()))?;
            batch
        };
        for product in &family.needs {
            if let BuildResult::Failed { output, .. } = ctx.builds.ensure(tree, product, &[]) {
                return Err(build_failure(
                    &format!("the build product {product} failed"),
                    &output,
                ));
            }
        }
        let file = self
            .settings
            .outputs
            .join(format!("explore-{name}-{}.jsonl", std::process::id()));
        fs::create_dir_all(&self.settings.outputs)?;
        let _ = fs::remove_file(&file);
        let run_command = family.run.as_deref().unwrap_or_default();
        let names: Vec<&CheckName> = batch.iter().map(|c| &c.check).collect();
        let env = vec![(adapters::RESULTS_ENV.to_owned(), file.display().to_string())];
        let run = ctx.launcher.run(&self.command(
            adapters::jsonl_run(run_command, &names),
            path,
            env,
            tree,
            family,
        ));
        let reported = adapters::jsonl_results(&fs::read_to_string(&file).unwrap_or_default());
        let _ = fs::remove_file(&file);
        if reported.is_empty() {
            // Nothing reported means the tool itself failed; the batch stays saved and runs
            // again next time.
            self.add_spent(name, &run.usage)?;
            return Err(Error::Io(format!(
                "{name}: the run reported no check; its output is in {}",
                run.output.display()
            )));
        }
        let output = tail(&run.output);
        let finds: Vec<Report> = batch
            .iter()
            .filter_map(|check| {
                let reported = reported.iter().find(|r| r.check == check.check)?;
                (reported.outcome == Outcome::Fail).then(|| {
                    Report::Find(ExplorationFind {
                        check: check.check.clone(),
                        machine: ctx.machine.clone(),
                        host: self.settings.hostname.clone(),
                        commit: commit.clone(),
                        replay: adapters::jsonl_run(run_command, &[&check.check]).join(" "),
                        output: failure_text(reported, &output),
                    })
                })
            })
            .collect();
        let day = self.clock.now().day();
        let instructions = billions(&run.usage);
        self.update(|state| {
            state
                .played
                .entry(name.to_owned())
                .or_default()
                .extend(batch.iter().map(NewCheck::identity));
            state.batches.remove(name);
            state.unsent.extend(finds);
            *state
                .spent
                .entry(name.to_owned())
                .or_default()
                .entry(day)
                .or_default() += instructions;
        })?;
        Ok(!family.needs.is_empty())
    }

    /// Asks the family's `next` command for checks, keeping those not played before.
    fn ask_next(
        &self,
        ctx: &Context,
        name: &str,
        family: &Family,
        tree: &TreeId,
        path: &Path,
    ) -> Result<Vec<NewCheck>> {
        let next = family.next.as_deref().unwrap_or_default();
        let run =
            ctx.launcher
                .run(&self.command(adapters::words(next), path, Vec::new(), tree, family));
        self.add_spent(name, &run.usage)?;
        if run.exit != Some(0) {
            return Err(Error::Io(format!(
                "{name}: `next` exited with {:?}; its output is in {}",
                run.exit,
                run.output.display()
            )));
        }
        let text = fs::read_to_string(&run.output).unwrap_or_default();
        let played = self.state().played.get(name).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str::<NewCheck>(line.trim()).ok())
            .filter(|check| check.check.family() == name)
            .filter(|check| {
                let identity = check.identity();
                !played.contains(&identity) && seen.insert(identity)
            })
            .collect())
    }

    fn command(
        &self,
        args: Vec<String>,
        cwd: &Path,
        env: Vec<(String, String)>,
        tree: &TreeId,
        family: &Family,
    ) -> CommandSpec {
        CommandSpec {
            args,
            cwd: cwd.to_path_buf(),
            env,
            class: Class::Idle,
            cores: self.settings.cores,
            resources: family.resources.clone(),
            grows: family.grows,
            tree: Some(tree.clone()),
            ticket: None,
            hang_limit_s: None,
            reserve: None,
        }
    }

    fn add_spent(&self, family: &str, usage: &Usage) -> Result<()> {
        let day = self.clock.now().day();
        let cutoff = self.cutoff();
        self.update(|state| {
            let days = state.spent.entry(family.to_owned()).or_default();
            *days.entry(day).or_default() += billions(usage);
            days.retain(|day, _| *day >= cutoff);
        })
    }

    /// The first day inside the spending window.
    fn cutoff(&self) -> String {
        self.clock.now().plus_seconds(-WINDOW_DAYS * 86_400.0).day()
    }
}

/// A run's instructions in billions; zero where the system does not count them.
#[allow(clippy::cast_precision_loss)]
fn billions(usage: &Usage) -> f64 {
    usage.instructions.map_or(0.0, |n| n as f64 / 1e9)
}

/// The last lines of a saved output.
pub(super) fn tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n")
}

/// What a report says about a failed check: where it failed, its message and the run's output.
fn failure_text(reported: &Reported, output: &str) -> String {
    let mut text = String::new();
    for line in [&reported.at, &reported.message].into_iter().flatten() {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(output);
    text
}
