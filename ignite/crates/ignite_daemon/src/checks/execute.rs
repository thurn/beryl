//! Running a family's checks: the builds they need, the immediate rerun of a failure, the
//! retries of an unreported check, the rerun of another machine's cached failure, and storing
//! and recording what the runs said.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ignite_core::ids::{CheckName, Commit, RecordId};
use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family};
use ignite_core::protocol::hub::{
    CachedResult, CompileUnit, FlakyReport, IncompleteReason, NewResult, Record, ResultCost,
};
use ignite_core::protocol::{Outcome, Usage};
use ignite_core::traits::{BuildFor, BuildResult, CommandSpec, Error, RunResult};

use super::{
    Built, CheckRun, Checks, Failure, Final, Keyed, Listed, OpenTree, Reported, RunOptions, TRIES,
    Target, adapters, base_env, failure, hang_limit_s, incomplete, path_text,
};
use crate::server::Context;
use crate::supervise;

/// Whom a command's builds are for: its class, ticket and session.
pub(super) fn build_for(options: &RunOptions) -> BuildFor {
    BuildFor {
        class: options.class,
        ticket: options.ticket.clone(),
        session: options.session.clone(),
    }
}

/// The variable that tells each check command its class, so a family can wait longer for a slow
/// tool outside the landing gate.
pub const CLASS_ENV: &str = "IGNITE_CLASS";

impl Checks {
    /// The areas of the change in `tree`: what its branch changed since it left `staging`.
    /// Empty at the tip, and when the change cannot be read, so nothing is counted wrongly.
    pub(super) fn change_areas(&self, ctx: &Context, tree: &OpenTree) -> Vec<String> {
        let base = format!(
            "{}/{}",
            ctx.config.repository.remote, ctx.config.repository.staging
        );
        let changed = self.trees.changed(&tree.path, &base).unwrap_or_default();
        super::plan::areas(&changed, &tree.graph)
    }

    /// Answers each check from its cached result or runs it, with the rerun of a failure and
    /// the retries of an incomplete run, and stores what it learns. With `fail_fast`, as in
    /// the landing gate, no further command starts once a check's failure is confirmed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        items: Vec<(Keyed, Option<CachedResult>)>,
        options: &RunOptions,
        run: &mut CheckRun,
        fail_fast: bool,
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
                        message: Some("failed before on these inputs".to_owned()),
                        output: hit.output.clone(),
                        reason: None,
                    });
                    run.finish(&keyed.listed.check, hit.outcome, failure);
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
        let mut packages: Vec<String> = pending
            .iter()
            .filter_map(|k| k.listed.package.clone())
            .collect();
        packages.sort();
        packages.dedup();
        if Self::build(ctx, tree, family, &packages, &build_for(options), run).is_err() {
            for keyed in &pending {
                let check = &keyed.listed.check;
                let error = Error::Io(super::BUILD_FAILED.to_owned());
                let failure = Failure {
                    reason: Some(IncompleteReason::BuildFailed),
                    ..incomplete(check, &error)
                };
                run.finish(check, Outcome::Incomplete, Some(failure));
            }
            return;
        }
        run.ran += u32::try_from(pending.len()).unwrap_or(u32::MAX);
        let finals = self.attempts(ctx, tree, name, family, pending, options, run, fail_fast);
        self.settle(ctx, tree, family, finals, run);
    }

    /// Runs `pending` until each has a verdict: a failure runs once more at once, and a check
    /// left unreported runs at most [`TRIES`] times. A check its command says cannot run on this
    /// machine, and every check a hung command left unreported, is incomplete at once, because
    /// running it again here would end the same way. With `fail_fast`, once a failure is
    /// confirmed by its rerun, the checks still waiting for another run are left unanswered and
    /// counted in `run.skipped`.
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
        fail_fast: bool,
    ) -> Vec<Final> {
        // A first failure, with its run's output and record.
        let mut failed: BTreeMap<CheckName, (Reported, PathBuf, Option<RecordId>)> =
            BTreeMap::new();
        let mut tries: BTreeMap<CheckName, u32> = BTreeMap::new();
        // What each check's runs have cost so far: an equal share of every run it was in.
        let mut costs: BTreeMap<CheckName, ResultCost> = BTreeMap::new();
        let mut finals: Vec<Final> = Vec::new();
        while !pending.is_empty() {
            let (reported, output, record) =
                self.run_family(ctx, tree, name, family, &pending, options);
            run.wall_s += output.usage.wall_s;
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
            if fail_fast && !pending.is_empty() && finals.iter().any(|f| f.1 == Outcome::Fail) {
                run.skipped += u32::try_from(pending.len()).unwrap_or(u32::MAX);
                pending.clear();
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
        finals: Vec<Final>,
        run: &mut CheckRun,
    ) {
        let mut stored = Vec::new();
        let moved = moved_inputs(ctx, tree, &finals);
        let areas = self.change_areas(ctx, tree);
        for (keyed, outcome, failure, output, cost, record) in finals {
            if let Some(id) = record
                && outcome != Outcome::Incomplete
            {
                run.records.push((keyed.listed.check.clone(), id));
            }
            match outcome {
                // A result whose inputs changed while the check ran says nothing about the
                // hash it was keyed by, so it is reported to the caller but not stored.
                Outcome::Pass | Outcome::Fail
                    if keyed.inputs.iter().any(|(path, _)| moved.contains(path)) => {}
                Outcome::Pass | Outcome::Fail => stored.push(NewResult {
                    key: keyed.key.clone(),
                    outcome,
                    machine: ctx.machine.clone(),
                    // 0 when the hub has not numbered the run's record: it was queued.
                    record: record.unwrap_or(RecordId(0)),
                    output: output.map(|o| o.display().to_string()),
                    cost,
                    areas: areas.clone(),
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
        who: &BuildFor,
        run: &mut CheckRun,
    ) -> std::result::Result<(), PathBuf> {
        Self::build_products(ctx, tree, &family.needs, packages, who, run)
    }

    /// Makes each of `products` current, in the class of the command `who` names and charged
    /// to its ticket and session. `Err` holds a failed build's output.
    pub(super) fn build_products(
        ctx: &Context,
        tree: &OpenTree,
        products: &[String],
        packages: &[String],
        who: &BuildFor,
        run: &mut CheckRun,
    ) -> std::result::Result<(), PathBuf> {
        for product in products {
            match ctx.builds.ensure_for(&tree.id, product, packages, who) {
                BuildResult::Current => {}
                BuildResult::Built { usage, units } => run.builds.push(Built {
                    product: product.clone(),
                    wall_s: usage.wall_s,
                    rebuilt: units.iter().filter(|u| u.rebuilt).count(),
                }),
                BuildResult::Failed { output, .. } => {
                    // Every family that needs a product gets its remembered failure, which
                    // the run reports once.
                    if !run.build_failures.iter().any(|(p, _)| p == product) {
                        run.build_failures.push((product.clone(), output.clone()));
                    }
                    return Err(output);
                }
            }
        }
        Ok(())
    }

    /// Runs the family's `run` command once for `checks` and reads what it reported, with the
    /// ID the hub gave the run's record if it took it.
    pub(super) fn run_family(
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
                let (result, record) = self
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
                let args = adapters::nextest_run_command(family.run.as_deref(), &targets);
                let mut env = vec![(
                    adapters::NEXTEST_JSON_ENV.0.to_owned(),
                    adapters::NEXTEST_JSON_ENV.1.to_owned(),
                )];
                env.extend(self.unit_recording_env());
                let cwd = self.nextest_dir(tree, family);
                let (result, record) =
                    self.launch_recorded(ctx, tree, name, family, RUN, args, &cwd, env, options);
                let text = fs::read_to_string(&result.output).unwrap_or_default();
                let listed: Vec<Listed> = checks.iter().map(|k| k.listed.clone()).collect();
                (adapters::nextest_results(&text, &listed), result, record)
            }
        }
    }

    /// The variables that make Cargo's target runner (and the compiler wrapper) record each test
    /// process (and compile) of a `cargo nextest run` into a new units file, as the daemon's own
    /// builds do (`crate::wrap::cargo_env`). [`Checks::launch_recorded`] reads the file back and
    /// attaches its units to the command's record. Empty when this daemon has no `ignite`
    /// executable to point the scripts at, or cannot write them: the run then goes unrecorded
    /// one by one but is otherwise unchanged.
    fn unit_recording_env(&self) -> Vec<(String, String)> {
        if self.settings.ignite_exe.as_os_str().is_empty() {
            return Vec::new();
        }
        if let Err(error) =
            crate::wrap::write_shims(&self.settings.shims, &self.settings.ignite_exe)
                .and_then(|()| fs::create_dir_all(&self.settings.outputs))
        {
            eprintln!(
                "ignite: cannot write the test runner to {}: {error}",
                self.settings.shims.display()
            );
            return Vec::new();
        }
        let units = self.settings.outputs.join(format!(
            "units-{}-{}.jsonl",
            std::process::id(),
            self.next_file.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&units);
        crate::wrap::cargo_env(&self.settings.shims, &units)
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
    ) -> RunResult {
        self.launch_recorded(ctx, tree, name, family, command, args, cwd, env, options)
            .0
    }

    /// [`Checks::launch`], with the ID the hub gave the command's record, if it took it.
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
    ) -> (RunResult, Option<RecordId>) {
        env.push((CLASS_ENV.to_owned(), options.class.as_str().to_owned()));
        let spec_env = env.clone();
        let slowest = self.cache.slowest_pass(name, command);
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
            hang_limit_s: Some(hang_limit_s(slowest)),
        });
        // The units the command's test processes and compiles recorded, if it was set up to.
        let units = env_units_file(&spec_env)
            .map(|file| {
                let units = crate::build::read_units(&file);
                let _ = fs::remove_file(&file);
                units
            })
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
        (result, record)
    }

    /// Sends a record of one command to the hub, or queues it when the hub cannot take it.
    /// Returns the ID the hub gave the record, if it took it.
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
        compile_units: Vec<CompileUnit>,
    ) -> Option<RecordId> {
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
            compile_units,
        };
        self.cache.record(&record)
    }
}

/// The units file a command's environment names, if it names one.
fn env_units_file(env: &[(String, String)]) -> Option<PathBuf> {
    env.iter()
        .find(|(name, _)| name == crate::wrap::UNITS_ENV)
        .map(|(_, file)| PathBuf::from(file))
}

/// The input paths of `finals` whose git object IDs differ from the ones their hashes were
/// computed from: the tree was edited while the checks ran. The inputs are read again in one
/// request; if that fails, every input counts as moved, because nothing can be vouched for.
fn moved_inputs(ctx: &Context, tree: &OpenTree, finals: &[Final]) -> BTreeSet<String> {
    let before: BTreeMap<&str, &str> = finals
        .iter()
        .flat_map(|(keyed, ..)| &keyed.inputs)
        .map(|(path, oid)| (path.as_str(), oid.as_str()))
        .collect();
    let paths: Vec<String> = before.keys().map(|p| (*p).to_owned()).collect();
    match inputs::object_ids(ctx.git.as_ref(), &tree.path, &paths) {
        Ok(now) => now
            .into_iter()
            .filter(|(path, oid)| before.get(path.as_str()) != Some(&oid.as_str()))
            .map(|(path, _)| path)
            .collect(),
        Err(_) => paths.into_iter().collect(),
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

/// The two commands of a family, as pass times name them.
pub(super) const LIST: &str = "list";
pub(super) const RUN: &str = "run";

/// What an incomplete check's failure says: why it gave up, with what the command said.
fn incomplete_message(
    reason: IncompleteReason,
    said: Option<String>,
    machine: &ignite_core::ids::MachineId,
) -> String {
    let why = match reason {
        IncompleteReason::Unreported => format!("not reported after {TRIES} tries"),
        IncompleteReason::CannotRun => format!("cannot run on {machine}"),
        IncompleteReason::Hung => {
            "hung: ended at its hang limit; its output lists the processes it left".to_owned()
        }
        IncompleteReason::BuildFailed => super::BUILD_FAILED.to_owned(),
        IncompleteReason::InvalidEntry => "its manifest entry is invalid".to_owned(),
    };
    match said {
        Some(said) => format!("{why}: {said}"),
        None => why,
    }
}
