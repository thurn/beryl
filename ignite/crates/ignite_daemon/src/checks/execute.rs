//! Running a family's checks: the builds they need, the immediate rerun of a failure, the
//! retries of an unreported check, and storing and recording what the runs said.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ignite_core::ids::{CheckName, Commit, RecordId};
use ignite_core::manifest::{Adapter, Family};
use ignite_core::protocol::hub::{CachedResult, FlakyReport, NewResult, Record};
use ignite_core::protocol::{Outcome, Usage};
use ignite_core::traits::{BuildResult, CommandSpec, Error, RunResult};

use super::{
    Built, CheckRun, Checks, Failure, Final, Keyed, Listed, OpenTree, Reported, RunOptions, TRIES,
    Target, adapters, base_env, failure, incomplete, path_text,
};
use crate::server::Context;

impl Checks {
    /// Answers each check from its cached result or runs it, with the rerun of a failure and
    /// the retries of an incomplete run, and stores what it learns.
    pub(crate) fn execute(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        items: Vec<(Keyed, Option<CachedResult>)>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) {
        let Some(family) = self.settings.manifest.families.get(name).map(|d| &d.family) else {
            return;
        };
        let mut pending: Vec<Keyed> = Vec::new();
        let listed = items.len();
        // The tree's commit, read once and only when a hit needs it.
        let mut head: Option<Option<Commit>> = None;
        for (keyed, hit) in items {
            match hit {
                Some(hit) => {
                    run.from_cache += 1;
                    if let Some(sink) = self.hits.get()
                        && let Some(commit) =
                            head.get_or_insert_with(|| ignite_git::refs::head(&tree.path).ok())
                    {
                        sink.hit(&hit, commit);
                    }
                    let failure = (hit.outcome == Outcome::Fail).then(|| Failure {
                        check: keyed.listed.check.clone(),
                        outcome: Outcome::Fail,
                        at: None,
                        message: Some("failed before on these inputs".to_owned()),
                        output: hit.output.clone(),
                    });
                    run.finish(&keyed.listed.check, hit.outcome, failure);
                }
                None => pending.push(keyed),
            }
        }
        if pending.is_empty() {
            if listed > 0 {
                self.record(ctx, tree, name, options, &Usage::default(), "cached", None);
            }
            return;
        }
        let mut packages: Vec<String> = pending
            .iter()
            .filter_map(|k| k.listed.package.clone())
            .collect();
        packages.sort();
        packages.dedup();
        if Self::build(ctx, tree, family, &packages, run).is_err() {
            for keyed in &pending {
                let check = &keyed.listed.check;
                let error = Error::Io("a build it needs failed".to_owned());
                run.finish(check, Outcome::Incomplete, Some(incomplete(check, &error)));
            }
            return;
        }
        run.ran += u32::try_from(pending.len()).unwrap_or(u32::MAX);
        let finals = self.attempts(ctx, tree, name, family, pending, options, run);
        self.settle(ctx, tree, family, finals, run);
    }

    /// Runs `pending` until each has a verdict: a failure runs once more at once, and a check
    /// left unreported runs at most [`TRIES`] times.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn attempts(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        mut pending: Vec<Keyed>,
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Final> {
        let mut failed: BTreeMap<CheckName, (Reported, PathBuf)> = BTreeMap::new();
        let mut tries: BTreeMap<CheckName, u32> = BTreeMap::new();
        let mut finals: Vec<Final> = Vec::new();
        while !pending.is_empty() {
            let (reported, output) = self.run_family(ctx, tree, name, family, &pending, options);
            run.wall_s += output.usage.wall_s;
            let mut again = Vec::new();
            for keyed in pending {
                let check = keyed.listed.check.clone();
                let report = reported.iter().find(|r| r.check == check);
                match report.map(|r| r.outcome) {
                    Some(Outcome::Pass | Outcome::Flaky) => match failed.remove(&check) {
                        // A flaky check carries the failed run's output.
                        Some((_, first_output)) => {
                            finals.push((keyed, Outcome::Flaky, None, Some(first_output)));
                        }
                        None => finals.push((keyed, Outcome::Pass, None, None)),
                    },
                    Some(Outcome::Fail) if failed.contains_key(&check) => {
                        let (report, first_output) = failed
                            .remove(&check)
                            .unwrap_or_else(|| unreachable!("a failed check is in the map"));
                        let failure = failure(&report, &first_output);
                        finals.push((keyed, Outcome::Fail, Some(failure), Some(first_output)));
                    }
                    Some(Outcome::Fail) => {
                        let report = report.cloned().unwrap_or_else(|| unreachable!());
                        failed.insert(check, (report, output.output.clone()));
                        again.push(keyed);
                    }
                    Some(Outcome::Incomplete) | None => {
                        let count = tries.entry(check.clone()).or_insert(0);
                        *count += 1;
                        if *count < TRIES {
                            again.push(keyed);
                        } else if let Some((report, first_output)) = failed.remove(&check) {
                            // It failed on these inputs and no later run said otherwise.
                            let failure = failure(&report, &first_output);
                            finals.push((keyed, Outcome::Fail, Some(failure), Some(first_output)));
                        } else {
                            let failure = Failure {
                                check: check.clone(),
                                outcome: Outcome::Incomplete,
                                at: None,
                                message: Some(
                                    report
                                        .and_then(|r| r.message.clone())
                                        .unwrap_or(format!("not reported after {TRIES} tries")),
                                ),
                                output: Some(output.output.display().to_string()),
                            };
                            finals.push((keyed, Outcome::Incomplete, Some(failure), None));
                        }
                    }
                }
            }
            pending = again;
        }
        finals
    }

    /// Stores passes and failures, reports flaky checks and adds every outcome to `run`.
    ///
    /// A flaky report names its command by the machine and the path of the failed run's saved
    /// output, which is the same for every check that failed in that run, so the hub can tell
    /// one fault of the machine that made many checks flake from flaky checks.
    pub(super) fn settle(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        finals: Vec<Final>,
        run: &mut CheckRun,
    ) {
        let mut stored = Vec::new();
        for (keyed, outcome, failure, output) in finals {
            match outcome {
                Outcome::Pass | Outcome::Fail => stored.push(NewResult {
                    key: keyed.key.clone(),
                    outcome,
                    machine: ctx.machine.clone(),
                    // The hub links costs through records, whose IDs the `Hub` trait does not
                    // return; 0 stands for an unknown record.
                    record: RecordId(0),
                    output: output.map(|o| o.display().to_string()),
                }),
                Outcome::Flaky => {
                    if let Ok(commit) = self.trees.head(&tree.path) {
                        let report = FlakyReport {
                            check: keyed.key.check.clone(),
                            hash: keyed.key.hash.clone(),
                            commit,
                            machine: ctx.machine.clone(),
                            always: family.always,
                            command: output.as_ref().map_or_else(String::new, |o| {
                                format!("{}:{}", ctx.machine, o.display())
                            }),
                            output: output.as_deref().map(output_tail).unwrap_or_default(),
                        };
                        self.cache.flaky(&report);
                    }
                }
                Outcome::Incomplete => {}
            }
            run.finish(&keyed.listed.check, outcome, failure);
        }
        self.cache.store(&stored);
    }

    /// Makes each product the family needs current. `Err` holds a failed build's output.
    pub(super) fn build(
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        packages: &[String],
        run: &mut CheckRun,
    ) -> std::result::Result<(), PathBuf> {
        for product in &family.needs {
            match ctx.builds.ensure(&tree.id, product, packages) {
                BuildResult::Current => {}
                BuildResult::Built { usage, units } => run.builds.push(Built {
                    product: product.clone(),
                    wall_s: usage.wall_s,
                    rebuilt: units.iter().filter(|u| u.rebuilt).count(),
                }),
                BuildResult::Failed { output, .. } => {
                    run.build_failures.push((product.clone(), output.clone()));
                    return Err(output);
                }
            }
        }
        Ok(())
    }

    /// Runs the family's `run` command once for `checks` and reads what it reported.
    pub(super) fn run_family(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        checks: &[Keyed],
        options: &RunOptions,
    ) -> (Vec<Reported>, RunResult) {
        match family.adapter {
            Adapter::Jsonl => {
                let file = self.settings.outputs.join(format!(
                    "results-{}-{}.jsonl",
                    std::process::id(),
                    self.next_file.fetch_add(1, Ordering::Relaxed)
                ));
                let _ = fs::create_dir_all(&self.settings.outputs);
                let _ = fs::remove_file(&file);
                let names: Vec<&CheckName> = checks.iter().map(|k| &k.listed.check).collect();
                let args = adapters::jsonl_run(family.run.as_deref().unwrap_or_default(), &names);
                let mut env = vec![(adapters::RESULTS_ENV.to_owned(), path_text(&file))];
                env.extend(base_env(ctx));
                let result = self.launch(ctx, tree, name, family, args, &tree.path, env, options);
                let text = fs::read_to_string(&file).unwrap_or_default();
                let _ = fs::remove_file(&file);
                (adapters::jsonl_results(&text), result)
            }
            Adapter::Nextest => {
                let targets: Vec<&Target> = checks
                    .iter()
                    .filter_map(|k| k.listed.target.as_ref())
                    .collect();
                let args = adapters::nextest_run_command(family.run.as_deref(), &targets);
                let env = vec![(
                    adapters::NEXTEST_JSON_ENV.0.to_owned(),
                    adapters::NEXTEST_JSON_ENV.1.to_owned(),
                )];
                let cwd = self.nextest_dir(tree, family);
                let result = self.launch(ctx, tree, name, family, args, &cwd, env, options);
                let text = fs::read_to_string(&result.output).unwrap_or_default();
                let listed: Vec<Listed> = checks.iter().map(|k| k.listed.clone()).collect();
                (adapters::nextest_results(&text, &listed), result)
            }
        }
    }

    /// Where cargo-nextest runs: the directory of the first product the family needs.
    pub(super) fn nextest_dir(&self, tree: &OpenTree, family: &Family) -> PathBuf {
        family
            .needs
            .first()
            .and_then(|n| self.settings.manifest.products.get(n))
            .and_then(|p| p.dir.as_deref())
            .map_or_else(|| tree.path.clone(), |dir| tree.path.join(dir))
    }

    /// Runs one command of the family through the launcher and records its cost.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn launch(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        args: Vec<String>,
        cwd: &Path,
        env: Vec<(String, String)>,
        options: &RunOptions,
    ) -> RunResult {
        let result = ctx.launcher.run(&CommandSpec {
            args,
            cwd: cwd.to_path_buf(),
            env,
            class: options.class,
            cores: self.settings.cores,
            resources: family.resources.clone(),
            grows: family.grows,
            tree: Some(tree.id.clone()),
            ticket: options.ticket.clone(),
        });
        let outcome = (result.exit == Some(0)).then_some(Outcome::Pass);
        self.record(
            ctx,
            tree,
            name,
            options,
            &result.usage,
            &options.verb,
            outcome,
        );
        result
    }

    /// Sends a record of one command to the hub, or queues it when the hub cannot take it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &str,
        options: &RunOptions,
        usage: &Usage,
        verb: &str,
        outcome: Option<Outcome>,
    ) {
        #[allow(clippy::cast_precision_loss)]
        let record = Record {
            machine: ctx.machine.clone(),
            ticket: options.ticket.clone(),
            session: options.session.clone(),
            tree: Some(tree.id.clone()),
            verb: verb.to_owned(),
            class: options.class,
            family: Some(family.to_owned()),
            queued_s: usage.queued_s,
            wall_s: usage.wall_s,
            cpu_s: usage.cpu_s,
            instructions: usage.instructions.map(|n| n as f64),
            peak_mb: usage.peak_mb,
            outcome,
            commit: None,
            ignite: self.settings.ignite.clone(),
            at: ctx.clock.now(),
            compile_units: Vec::new(),
        };
        self.cache.record(&record);
    }
}

/// How many lines of a failed run's output a flaky report carries.
const FLAKY_OUTPUT_LINES: usize = 40;

/// The last lines of a saved output; empty if it cannot be read.
fn output_tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(FLAKY_OUTPUT_LINES)..].join("\n")
}
