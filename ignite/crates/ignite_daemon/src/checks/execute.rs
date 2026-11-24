//! Running a family's checks: the builds they need, the immediate rerun of a failure, the
//! retries of an unreported check, the rerun of another machine's cached failure, and storing
//! and recording what the runs said.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use ignite_core::ids::{CheckName, Commit, RecordId};
use ignite_core::manifest::{Adapter, Family};
use ignite_core::protocol::hub::{
    CachedResult, FlakyReport, IncompleteReason, NewResult, ResultCost,
};
use ignite_core::protocol::{Class, Outcome, Usage};
use ignite_core::traits::{BuildFor, CommandSpec, Error, Reserve, RunResult};

use super::prepare::moved_inputs;
use super::units::CompileSpan;
use crate::build::repair;

use super::{
    CheckRun, Checks, Failure, Final, Keyed, Listed, OpenTree, Reported, RunOptions, TRIES, Target,
    adapters, base_env, failure, hang_limit_s, incomplete, incomplete_message, path_text, units,
};
use crate::server::Context;
use crate::supervise;

/// A verb process can read the result before the daemon tells it the verb was stopped.
/// The supervisor marks the run before sending SIGTERM, which nextest can report as failure.
pub(super) fn interrupted(output: &RunResult) -> bool {
    crate::steps::stop_requested()
        || (output.exit != Some(0) && supervise::ResultFile::ended_from_outside(&output.output))
}

/// Whom a command's builds are for: its class, ticket and session.
pub(super) fn build_for(options: &RunOptions) -> BuildFor {
    BuildFor {
        class: options.class,
        ticket: options.ticket.clone(),
        session: options.session.clone(),
    }
}

/// Whether the verb was stopped or the landing gate found a confirmed failure.
fn stopped(stop: Option<&AtomicBool>) -> bool {
    crate::steps::stop_requested() || stop.is_some_and(|stop| stop.load(Ordering::SeqCst))
}

/// The variable that tells each check command its class, so a family can wait longer for a slow
/// tool outside the landing gate.
pub const CLASS_ENV: &str = "IGNITE_CLASS";

impl Checks {
    /// The areas of the change in `tree`: what its branch changed since it left `staging`.
    /// Empty at the tip, and when the change cannot be read, so nothing is counted wrongly.
    pub(super) fn change_areas(&self, ctx: &Context, tree: &OpenTree) -> Vec<String> {
        let changed = super::verbs::changed_paths(self, ctx, tree).unwrap_or_default();
        super::plan::areas(&changed, &tree.graph)
    }

    /// Answers each check from its cached result or runs it, with the rerun of a failure and
    /// the retries of an incomplete run, and stores what it learns. With `stop`, as in the
    /// landing gate, no further command starts once a check's failure is confirmed, in this
    /// family or in any other family that shares the flag.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        items: Vec<(Keyed, Option<CachedResult>)>,
        options: &RunOptions,
        run: &mut CheckRun,
        stop: Option<&AtomicBool>,
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
                // A failure from another machine is not trusted to block: the check runs here,
                // and a pass, once stored, replaces it and makes the hub file a ticket for
                // that machine.
                Some(hit) if hit.outcome == Outcome::Fail && hit.machine != ctx.machine => {
                    pending.push(keyed);
                }
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
                        message: Some(crate::cache::FAILED_BEFORE.to_owned()),
                        output: hit.output.clone(),
                        reason: None,
                    });
                    if hit.outcome == Outcome::Fail {
                        run.failed_keys.push(keyed.key.clone());
                    }
                    run.finish_keyed(&keyed, family, hit.outcome, failure);
                }
                None => pending.push(keyed),
            }
        }
        if pending.is_empty() {
            if listed > 0 {
                self.record(
                    ctx,
                    tree,
                    name,
                    options,
                    &Usage::default(),
                    "cached",
                    None,
                    Vec::new(),
                );
            }
            return;
        }
        if stopped(stop) {
            run.skipped += u32::try_from(pending.len()).unwrap_or(u32::MAX);
            return;
        }
        let mut packages: Vec<String> = pending
            .iter()
            .filter_map(|k| k.listed.package.clone())
            .collect();
        packages.sort();
        packages.dedup();
        let products = super::products::hold_build_products(
            &tree.path,
            &family.needs,
            || {
                self.build(ctx, tree, family, &packages, &build_for(options), run)
                    .map_err(|_| Error::Io(super::BUILD_FAILED.to_owned()))
            },
            Error::from,
        );
        let _products = match products {
            Ok(guard) => guard,
            Err(error) => {
                for keyed in &pending {
                    let check = &keyed.listed.check;
                    let failure = Failure {
                        reason: Some(IncompleteReason::BuildFailed),
                        ..incomplete(check, &error)
                    };
                    run.finish(check, Outcome::Incomplete, Some(failure));
                }
                return;
            }
        };
        run.ran += u32::try_from(pending.len()).unwrap_or(u32::MAX);
        let finals = self.attempts(ctx, tree, name, family, pending, options, run, stop);
        self.settle(ctx, tree, family, finals, run);
    }

    /// Runs `pending` until each has a verdict: a failure runs once more at once, and a check
    /// left unreported runs at most [`TRIES`] times. A check its command says cannot run on this
    /// machine, and every check a hung command left unreported, is incomplete at once, because
    /// running it again here would end the same way. With `stop`, once a failure is confirmed
    /// by its rerun the flag is raised, and once it is raised, here or by another family, the
    /// checks still waiting for a run are left unanswered and counted in `run.skipped`.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn attempts(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        mut pending: Vec<Keyed>,
        options: &RunOptions,
        run: &mut CheckRun,
        stop: Option<&AtomicBool>,
    ) -> Vec<Final> {
        // A first failure, with its run's output and record.
        let mut failed: BTreeMap<CheckName, (Reported, PathBuf, Option<RecordId>)> =
            BTreeMap::new();
        let mut tries: BTreeMap<CheckName, u32> = BTreeMap::new();
        // What each check's runs have cost so far: an equal share of every run it was in.
        let mut costs: BTreeMap<CheckName, ResultCost> = BTreeMap::new();
        let mut finals: Vec<Final> = Vec::new();
        // Whether a damaged build was cleared and run again: that happens once.
        let mut repaired = false;
        while !pending.is_empty() {
            if stopped(stop) {
                run.skipped += u32::try_from(pending.len()).unwrap_or(u32::MAX);
                pending.clear();
                break;
            }
            let (reported, output, record, unbuilt) = self.run_family_built(
                ctx,
                tree,
                name,
                family,
                &pending,
                options,
                run,
                &mut repaired,
            );
            run.wall_s += output.usage.wall_s;
            // When the verb is stopped, nextest reports the signalled test as failed. Leave
            // this attempt unanswered, including any failures still waiting for confirmation.
            if interrupted(&output) {
                run.skipped += u32::try_from(pending.len()).unwrap_or(u32::MAX);
                break;
            }
            if !unbuilt && super::unstarted::looks_unstarted(&reported, &output, pending.len()) {
                // The cause is gone by the time anyone looks, so the machine is described now.
                let machine = super::unstarted::Machine::read(&tree.path);
                super::unstarted::record(&output, &reported, &machine);
            }
            if unbuilt {
                // Running it again would fail the same way: the checks wait on the build, which
                // the round reports once, and are never held off as if they had hung.
                let product = family.needs.first().map_or(name, String::as_str);
                if !run.build_failures.iter().any(|(p, _)| p == product) {
                    run.build_failures
                        .push((product.to_owned(), output.output.clone()));
                }
                let _ = supervise::keep(&output.output);
                let shares = pending.len();
                for keyed in pending.drain(..) {
                    let cost = costs.remove(&keyed.listed.check).unwrap_or_default();
                    let failure = Failure {
                        check: keyed.listed.check.clone(),
                        outcome: Outcome::Incomplete,
                        at: None,
                        message: Some(super::BUILD_FAILED.to_owned()),
                        output: Some(output.output.display().to_string()),
                        reason: Some(IncompleteReason::BuildFailed),
                    };
                    let cost = cost.plus(ResultCost::share_of(&output.usage, shares));
                    finals.push((
                        keyed,
                        Outcome::Incomplete,
                        Some(failure),
                        None,
                        cost,
                        record,
                    ));
                }
                break;
            }
            let mut judged = output.hung;
            let mut again = Vec::new();
            let shares = pending.len();
            for keyed in pending {
                let check = keyed.listed.check.clone();
                let cost = {
                    let total = costs.entry(check.clone()).or_default();
                    *total = total.plus(ResultCost::share_of(&output.usage, shares));
                    *total
                };
                let report = reported.iter().find(|r| r.check == check);
                let unfinished =
                    report.is_some_and(|r| r.outcome == Outcome::Incomplete && r.reason.is_none());
                let gave_up = match report.map(|r| (r.outcome, r.reason)) {
                    Some((Outcome::Pass, _)) => {
                        match failed.remove(&check) {
                            // A flaky check carries the failed run's output.
                            Some((_, first_output, first_record)) => {
                                finals.push((
                                    keyed,
                                    Outcome::Flaky,
                                    None,
                                    Some(first_output),
                                    cost,
                                    first_record,
                                ));
                            }
                            None => finals.push((keyed, Outcome::Pass, None, None, cost, record)),
                        }
                        continue;
                    }
                    // The adapter says the check flaked inside one run. That is reported as
                    // flaky and never cached, whether or not an earlier run failed.
                    Some((Outcome::Flaky, _)) => {
                        let first = failed.remove(&check);
                        let first_record = first.as_ref().map_or(record, |(_, _, r)| *r);
                        let first_output = first.map(|(_, output, _)| output);
                        finals.push((
                            keyed,
                            Outcome::Flaky,
                            None,
                            first_output,
                            cost,
                            first_record,
                        ));
                        continue;
                    }
                    Some((Outcome::Fail, _)) if failed.contains_key(&check) => {
                        let (report, first_output, first_record) = failed
                            .remove(&check)
                            .unwrap_or_else(|| unreachable!("a failed check is in the map"));
                        let failure = failure(&report, &first_output);
                        finals.push((
                            keyed,
                            Outcome::Fail,
                            Some(failure),
                            Some(first_output),
                            cost,
                            first_record,
                        ));
                        judged = true;
                        continue;
                    }
                    Some((Outcome::Fail, _)) => {
                        let report = report.cloned().unwrap_or_else(|| unreachable!());
                        failed.insert(check, (report, output.output.clone(), record));
                        again.push(keyed);
                        judged = true;
                        continue;
                    }
                    Some((Outcome::Incomplete, Some(reason @ IncompleteReason::CannotRun))) => {
                        reason
                    }
                    // A check whose last step passed before the command ended is run again
                    // alone, whatever ended the command, so it falls to the retries below.
                    _ if output.hung && !unfinished => IncompleteReason::Hung,
                    _ => {
                        judged = true;
                        let count = tries.entry(check.clone()).or_insert(0);
                        *count += 1;
                        if *count < TRIES {
                            again.push(keyed);
                            continue;
                        }
                        IncompleteReason::Unreported
                    }
                };
                judged = true;
                if let Some((report, first_output, first_record)) = failed.remove(&check) {
                    // It failed on these inputs and no later run said otherwise.
                    let failure = failure(&report, &first_output);
                    finals.push((
                        keyed,
                        Outcome::Fail,
                        Some(failure),
                        Some(first_output),
                        cost,
                        first_record,
                    ));
                    continue;
                }
                let said = report.and_then(|r| r.message.clone());
                let failure = Failure {
                    check: check.clone(),
                    outcome: Outcome::Incomplete,
                    at: None,
                    message: Some(incomplete_message(gave_up, said, &ctx.machine)),
                    output: Some(output.output.display().to_string()),
                    reason: Some(gave_up),
                };
                finals.push((
                    keyed,
                    Outcome::Incomplete,
                    Some(failure),
                    None,
                    cost,
                    record,
                ));
            }
            if judged {
                // The output of a failed, flaky or incomplete run is kept longer.
                let _ = supervise::keep(&output.output);
            }
            pending = again;
            if let Some(stop) = stop
                && finals.iter().any(|f| f.1 == Outcome::Fail)
            {
                stop.store(true, Ordering::SeqCst);
            }
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
        mut finals: Vec<Final>,
        run: &mut CheckRun,
    ) {
        let mut stored = Vec::new();
        let mut flaky = Vec::new();
        // A run that recorded what a test opened gives the test the hash its new list says.
        let unkeyed = self.rekey(ctx, tree, family, &mut finals);
        let moved = moved_inputs(ctx, tree, &finals);
        let areas = self.change_areas(ctx, tree);
        for (keyed, outcome, failure, output, cost, record) in finals {
            let inputs_moved = keyed.inputs.iter().any(|(path, _)| moved.contains(path));
            if inputs_moved && (outcome == Outcome::Flaky || family.machine_relative) {
                // These verdicts need the same inputs throughout the run.
                let check = &keyed.listed.check;
                let error = Error::Io("inputs changed during the run; run the check again".into());
                run.finish(check, Outcome::Incomplete, Some(incomplete(check, &error)));
                continue;
            }
            if let Some(id) = record
                && outcome != Outcome::Incomplete
            {
                run.records.push((keyed.listed.check.clone(), id));
            }
            match outcome {
                // A result whose inputs changed while the check ran says nothing about the
                // hash it was keyed by, so it is reported to the caller but not stored.
                Outcome::Pass | Outcome::Fail if inputs_moved => {}
                // A check whose hash does not cover what it opened says nothing about it either.
                Outcome::Pass | Outcome::Fail if unkeyed.contains(&keyed.listed.check) => {}
                Outcome::Pass | Outcome::Fail => {
                    if outcome == Outcome::Fail {
                        run.failed_keys.push(keyed.key.clone());
                    }
                    stored.push(NewResult {
                        key: keyed.key.clone(),
                        outcome,
                        machine: ctx.machine.clone(),
                        // 0 when the hub has not numbered the run's record: it was queued.
                        record: record.unwrap_or(RecordId(0)),
                        output: output.map(|o| o.display().to_string()),
                        cost,
                        areas: areas.clone(),
                    });
                }
                Outcome::Flaky => {
                    if let Ok(commit) = self.trees.head(&tree.path) {
                        flaky.push(FlakyReport {
                            check: keyed.key.check.clone(),
                            hash: keyed.key.hash.clone(),
                            commit,
                            machine: ctx.machine.clone(),
                            always: family.always,
                            command: output.as_ref().map_or_else(String::new, |o| {
                                format!("{}:{}", ctx.machine, o.display())
                            }),
                            output: output.as_deref().map(output_tail).unwrap_or_default(),
                        });
                    }
                }
                Outcome::Incomplete => {}
            }
            if family.builds && (inputs_moved || unkeyed.contains(&keyed.listed.check)) {
                // The verdict cannot satisfy a build listing unless it verifies that hash.
                run.finish(&keyed.listed.check, outcome, failure);
            } else {
                run.finish_keyed(&keyed, family, outcome, failure);
            }
        }
        self.cache.flaky(&flaky);
        self.cache.store(&stored);
    }

    /// Runs the family's `run` command for `checks` as [`Checks::run_family`] does, and says
    /// whether the Cargo build that a cargo-nextest run starts failed, so no check ran.
    ///
    /// A cargo-nextest run builds whatever is out of date before it tests. A build ended partway
    /// in the tree is cleared up first, and a build failure that looks like damage to the tree's
    /// build products (see `crate::build::repair`) is cleared and run once more, at most once
    /// per `repaired`. The first run's time is added to `run`.
    #[allow(clippy::too_many_arguments)]
    fn run_family_built(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        checks: &[Keyed],
        options: &RunOptions,
        run: &mut CheckRun,
        repaired: &mut bool,
    ) -> (Vec<Reported>, RunResult, Option<RecordId>, bool) {
        if family.adapter != Adapter::Nextest {
            let (reported, output, record) =
                self.run_family(ctx, tree, name, family, checks, options);
            return (reported, output, record, false);
        }
        let targets = self.target_dirs(tree, family);
        for target in &targets {
            repair::clear_if_suspect(target);
        }
        let (reported, output, record) = self.run_family(ctx, tree, name, family, checks, options);
        if repair::mark_if_ended(&output, &targets) {
            return (reported, output, record, false);
        }
        let failure = build_failure(&reported, &output);
        let Some(failure) = failure else {
            return (reported, output, record, false);
        };
        if !failure.damaged || *repaired {
            return (reported, output, record, true);
        }
        *repaired = true;
        run.wall_s += output.usage.wall_s;
        for target in &targets {
            repair::clear_crates(target, &failure.crates);
        }
        eprintln!(
            "ignite: the build of {name}'s checks failed in {}; running them once more after \
             clearing {}",
            tree.id,
            failure.crates.join(", ")
        );
        let (reported, output, record) = self.run_family(ctx, tree, name, family, checks, options);
        let ended = repair::mark_if_ended(&output, &targets);
        let unbuilt = !ended && build_failure(&reported, &output).is_some();
        (reported, output, record, unbuilt)
    }

    /// The target directories of the products `family` needs, in `tree`.
    fn target_dirs(&self, tree: &OpenTree, family: &Family) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = family
            .needs
            .iter()
            .filter_map(|n| self.settings.manifest.products.get(n))
            .flat_map(|p| p.outputs.iter().map(|o| tree.path.join(o)))
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Runs the family's `run` command once for `checks` and reads what it reported, with the
    /// ID the hub gave the run's record if it took it. Callers use `run_family` (`split.rs`),
    /// which splits a run that one command line cannot hold.
    pub(super) fn run_family_once(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        checks: &[Keyed],
        options: &RunOptions,
    ) -> (Vec<Reported>, RunResult, Option<RecordId>) {
        let _running = crate::steps::during(format!("running {} checks of {name}", checks.len()));
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
                let (result, record, _) = self
                    .launch_recorded(ctx, tree, name, family, RUN, args, &tree.path, env, options);
                let text = fs::read_to_string(&file).unwrap_or_default();
                let _ = fs::remove_file(&file);
                let reported = adapters::jsonl_results(&text);
                let cut_short = result.hung
                    || (result.exit != Some(0)
                        && !reported.iter().any(|r| r.outcome == Outcome::Fail));
                if cut_short {
                    return (adapters::jsonl_results_cut_short(&text), result, record);
                }
                (reported, result, record)
            }
            Adapter::Nextest => {
                let targets: Vec<&Target> = checks
                    .iter()
                    .filter_map(|k| k.listed.target.as_ref())
                    .collect();
                let mut packages: Vec<String> = checks
                    .iter()
                    .filter_map(|k| k.listed.package.clone())
                    .collect();
                packages.sort();
                packages.dedup();
                let packages = Self::selected(ctx, tree, family, &packages);
                let exclude = self.nextest_exclude(family);
                let args = adapters::nextest_run_command(
                    family.run.as_deref(),
                    &packages,
                    &exclude,
                    &targets,
                );
                let mut env = vec![(
                    adapters::NEXTEST_JSON_ENV.0.to_owned(),
                    adapters::NEXTEST_JSON_ENV.1.to_owned(),
                )];
                env.extend(self.unit_recording_env());
                // The tests record the repository files they open (see `run_reads`).
                let reads = self.reads_dir();
                if let Some(dir) = &reads {
                    env.push((super::run_reads::READS_ENV.to_owned(), path_text(dir)));
                }
                let cwd = self.nextest_dir(tree, family);
                let (result, record, _) =
                    self.launch_recorded(ctx, tree, name, family, RUN, args, &cwd, env, options);
                let text = fs::read_to_string(&result.output).unwrap_or_default();
                let listed: Vec<Listed> = checks.iter().map(|k| k.listed.clone()).collect();
                let reported = adapters::nextest_results(&text, &listed);
                if let Some(dir) = &reads {
                    self.note_run_reads(dir, name, checks, &reported);
                }
                (reported, result, record)
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

    /// The packages a `nextest` family's command selects to list or run the checks of
    /// `packages`: those the build of the first product it needs selects for them
    /// ([`ignite_core::traits::Builds::selected`]), so the command compiles nothing that build
    /// did not. None, standing for the whole workspace, when `packages` is empty.
    pub(super) fn selected(
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        packages: &[String],
    ) -> Vec<String> {
        match family.needs.first() {
            Some(product) if !packages.is_empty() => {
                ctx.builds.selected(&tree.id, product, packages)
            }
            _ => Vec::new(),
        }
    }

    /// The packages a `nextest` family's whole-workspace command leaves out: the `exclude` of
    /// the first product it needs, so that it selects what that product's build selects.
    pub(super) fn nextest_exclude(&self, family: &Family) -> Vec<String> {
        family
            .needs
            .first()
            .and_then(|n| self.settings.manifest.products.get(n))
            .map(|p| p.exclude.clone())
            .unwrap_or_default()
    }

    /// Runs one command of the family through the launcher, with its class in [`CLASS_ENV`] and
    /// a hang limit from its slowest pass, and records its cost. `command` is [`LIST`] or
    /// [`RUN`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn launch(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        command: &str,
        args: Vec<String>,
        cwd: &Path,
        env: Vec<(String, String)>,
        options: &RunOptions,
    ) -> (RunResult, Option<CompileSpan>) {
        let (result, _, span) =
            self.launch_recorded(ctx, tree, name, family, command, args, cwd, env, options);
        (result, span)
    }

    /// [`Checks::launch`], with the ID the hub gave the command's record, if it took it, and
    /// when the command's first compile started and its last one ended.
    #[allow(clippy::too_many_arguments)]
    fn launch_recorded(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        command: &str,
        args: Vec<String>,
        cwd: &Path,
        mut env: Vec<(String, String)>,
        options: &RunOptions,
    ) -> (RunResult, Option<RecordId>, Option<CompileSpan>) {
        env.push((CLASS_ENV.to_owned(), options.class.as_str().to_owned()));
        env.push(("IGNITE_MACHINE".to_owned(), ctx.machine.to_string()));
        env.push((supervise::HANG_ON_OUTPUT_IDLE.to_owned(), "1".to_owned()));
        let spec_env = env.clone();
        let slowest = self.cache.slowest_pass(name, command);
        let result = ctx.launcher.run(&CommandSpec {
            args,
            cwd: cwd.to_path_buf(),
            env,
            class: options.class,
            cores: cores_for(command, family.cores.unwrap_or(self.settings.cores)),
            resources: if command == LIST && family.list_without_resources {
                Vec::new()
            } else {
                family.resources.clone()
            },
            grows: family.grows,
            tree: Some(tree.id.clone()),
            ticket: options.ticket.clone(),
            hang_limit_s: Some(hang_limit_s(slowest)),
            reserve: (options.class == Class::Tip
                && ctx.config.repository.green.is_some()
                && family.builds)
                .then_some(Reserve::TipBuild),
        });
        // The units the command's test processes and compiles recorded, if it was set up to.
        let (units, span) = units::env_units_file(&spec_env)
            .map(|file| units::take(&file))
            .unwrap_or_default();
        let outcome = (result.exit == Some(0) && !result.hung).then_some(Outcome::Pass);
        if outcome.is_some() {
            self.cache.note_pass(name, command, result.usage.wall_s);
        }
        let record = self.record(
            ctx,
            tree,
            name,
            options,
            &result.usage,
            &options.verb,
            outcome,
            units,
        );
        (result, record, span)
    }
}

/// What a run's Cargo build said when it failed before any check ran (see
/// [`repair::run_failure`]).
fn build_failure(reported: &[Reported], output: &RunResult) -> Option<repair::CompileFailure> {
    let ran = reported
        .iter()
        .any(|r| matches!(r.outcome, Outcome::Pass | Outcome::Fail | Outcome::Flaky));
    repair::run_failure(ran, output)
}

/// How many lines of a failed run's output a flaky report carries.
const FLAKY_OUTPUT_LINES: usize = 40;

/// The last lines of a saved output; empty if it cannot be read.
fn output_tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(FLAKY_OUTPUT_LINES)..].join("\n")
}

/// The two commands of a family, as pass times name them.
pub(super) const LIST: &str = "list";
pub(super) const RUN: &str = "run";

/// The cores a check command asks the scheduler for. A `list` command only prints what the
/// family has, so it asks for one core and starts at once; a `run` command asks for the
/// family's `cores`, or else the settings' cores. A gate whose checks are all cached launches neither, so it asks for none.
fn cores_for(command: &str, run_cores: u32) -> u32 {
    if command == LIST { 1 } else { run_cores }
}
