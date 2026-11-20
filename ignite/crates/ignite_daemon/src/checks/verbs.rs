//! The verbs that run and explain checks: `ignite test`, `ignite check` and `ignite why`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use serde_json::json;

use ignite_core::ids::{CheckName, TicketId, TreeId};
use ignite_core::manifest::{Family, Kind};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::CachedResult;
use ignite_core::protocol::tickets::{Status, TicketQuery};
use ignite_core::protocol::{Class, Outcome};

use super::express::{Asked, express_reply};
use super::named::test_families;
pub(crate) use super::gate::gate_lines;
use super::gate::scope_line;
use super::plan::{self, GateView};
use super::time_lines::{Stopwatch, slow_line, took_line};
pub(crate) use super::time_lines::{built_lines, listed_line};
use super::why_record;
use super::{CheckRun, Checks, Keyed, OpenTree, Prepared, RunOptions};
use crate::land::express::ExpressLane;
use crate::server::{Context, Verb};

/// The most failures a reply lists; the rest are in its JSON.
const SHOWN_FAILURES: usize = 8;

/// `ignite test <names>`: runs the checks the agent names, in its tree. A name matches every
/// check whose name contains it, or, when it holds `*` or `?`, every check it matches as a
/// pattern. Names only in the express families run the express commands instead, in a tree
/// that changes only Ignite (see [`super::express`]).
pub struct TestVerb {
    pub checks: Arc<Checks>,
    /// The machine's express lane, if it has one.
    pub express: Option<Arc<dyn ExpressLane>>,
}

impl Verb for TestVerb {
    fn name(&self) -> &'static str {
        "test"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let (words, flags) = split_args(&req.args);
        if words.is_empty() {
            return Reply::text(Exit::Usage, "usage: ignite test <names> [--rerun]");
        }
        let options = options("test", req, flags.contains(&"--rerun"));
        let mut run = CheckRun::default();
        let started = Stopwatch::start(ctx);
        let (checks, tree) = match self.tree(ctx, req) {
            Ok(opened) => opened,
            Err(reply) => return reply,
        };
        run.stage(ctx, "opening the tree", started);
        let asked = Asked::Named(&words);
        if let Some(reply) = express_reply(self.express.as_ref(), &checks, ctx, &tree, asked) {
            return reply;
        }
        let finding = Stopwatch::start(ctx);
        let found = match test_families(&checks, ctx, &tree, &words, &mut run) {
            Ok(found) => found,
            Err(reply) => return reply,
        };
        let (families, scopes) = (&found.families, &found.scopes);
        run.stage(ctx, "finding families", finding);
        let listing = Stopwatch::start(ctx);
        let mut prepared =
            checks.prepare_scoped(ctx, &tree, families, scopes, &options, &mut run);
        run.stage(ctx, "listing", listing);
        let unmatched: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| {
                !prepared.iter().any(|p| {
                    p.keyed.iter().any(|k| matches(w, k.listed.check.as_str()))
                        || p.broken.iter().any(|f| matches(w, f.check.as_str()))
                })
            })
            .collect();
        if !unmatched.is_empty() && run.incomplete_families.is_empty() {
            return Reply::text(
                Exit::Usage,
                format!("no check matches {}", unmatched.join(", ")),
            );
        }
        for family in &mut prepared {
            family.keep(
                |check| words.iter().any(|w| matches(w, check.as_str())),
                &mut run,
            );
        }
        let running = Stopwatch::start(ctx);
        checks.run_prepared(ctx, &tree, prepared, &options, &mut run);
        run.stage(ctx, "running", running);
        let mut lines = built_lines(&run);
        lines.extend(listed_line(&run));
        lines.extend(found.skipped_line());
        lines.extend(slow_line(&run));
        lines.extend(took_line(&run, started.seconds(ctx)));
        lines.push(("ran".to_owned(), ran_line(&run)));
        lines.extend(failure_lines(&run));
        if let Some(first) = run.failures.first() {
            lines.push((
                "rerun".to_owned(),
                format!("ignite test --rerun {}", first.check),
            ));
        }
        reply(&run, render(&lines))
    }
}

/// `ignite check [--plan] [--rerun]`: runs what the landing gate would run on the tree as it is,
/// or with `--plan` prints which checks it would choose and why. In a tree that changes only
/// Ignite it runs the express commands, as the landing would (see [`super::express`]).
pub struct CheckVerb {
    pub checks: Arc<Checks>,
    /// The machine's express lane, if it has one.
    pub express: Option<Arc<dyn ExpressLane>>,
}

impl Verb for CheckVerb {
    fn name(&self) -> &'static str {
        "check"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let (_, flags) = split_args(&req.args);
        let options = options("check", req, flags.contains(&"--rerun"));
        let opening = Stopwatch::start(ctx);
        let (checks, tree) = match self.tree(ctx, req) {
            Ok(opened) => opened,
            Err(reply) => return reply,
        };
        if !flags.contains(&"--plan") {
            let lane = self.express.as_ref();
            if let Some(reply) = express_reply(lane, &checks, ctx, &tree, Asked::Gate) {
                return reply;
            }
            let mut gated = checks.gate_in(ctx, &tree, &options, None, &super::Excused::new());
            gated.opened(ctx, opening);
            let mut lines = built_lines(&gated.run);
            lines.extend(gate_lines(&gated));
            lines.extend(super::verdict::store_lines(ctx, &checks, &tree, &gated));
            return reply(&gated.run, render(&lines));
        }
        let regression: Vec<(&str, &Family)> = checks
            .families()
            .filter(|(_, f)| f.kind == Kind::Regression)
            .collect();
        let mut run = CheckRun::default();
        // Without the changed paths nothing can be left out of scope.
        let among = match changed_paths(&checks, ctx, &tree) {
            Ok(changed) => {
                let (among, left_out) = checks.scope(ctx, &tree, &regression, &changed);
                run.out_of_scope = left_out;
                among
            }
            Err(_) => regression,
        };
        let mut prepared = checks.prepare_among(ctx, &tree, &among, &options, &mut run);
        for family in &mut prepared {
            family.keep(|_| true, &mut run);
        }
        let hits = checks.lookup(&prepared, &options);
        let view = gate_view(&checks, ctx, &tree, &prepared, &hits);
        let gate_plan = view.plan(&ctx.config.gate);
        let mut lines = plan::plan_lines(&view, &gate_plan, &ctx.config.gate);
        lines.extend(failure_lines(&run));
        lines.extend(scope_line(&run));
        Reply {
            exit: Exit::Success,
            text: render(&lines),
            json: Some(json!({
                "run": gate_plan.run.iter().map(|p| p.check.as_str()).collect::<Vec<_>>(),
                "cached": gate_plan.cached.iter().map(|p| p.check.as_str()).collect::<Vec<_>>(),
                "past_budget": gate_plan.past_budget.len(),
                "cold": gate_plan.cold.len(),
                "tip": gate_plan.tip.len(),
                "budget_billions": view.budget(&ctx.config.gate).billions(),
                "rate": view.rate,
                "scored_cost": gate_plan.scored_cost,
                "forced_cost": gate_plan.forced_cost,
                "out_of_scope": run.out_of_scope,
            })),
            handle: None,
        }
    }
}

/// `ignite why <check> [--inputs]`: prints a check's record (its cost, its failures, where it
/// runs and why), or with `--inputs` what its input hash covers.
pub struct WhyVerb {
    pub checks: Arc<Checks>,
}

impl Verb for WhyVerb {
    fn name(&self) -> &'static str {
        "why"
    }

    /// It only reads, so it may explain a check while the tree is landing.
    fn waits_for_a_held_tree(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let (words, flags) = split_args(&req.args);
        let [wanted] = words.as_slice() else {
            return Reply::text(Exit::Usage, "usage: ignite why <check> [--inputs]");
        };
        let options = options("why", req, false);
        // Outside every build tree there is nothing to list or hash, but the record can
        // still be read.
        if ctx.tree.is_none() && self.checks.trees.tree_of(&req.cwd).is_none() {
            return why_record::outside_a_tree(&self.checks, wanted, flags.contains(&"--inputs"));
        }
        let (checks, tree) = match self.tree(ctx, req) {
            Ok(opened) => opened,
            Err(reply) => return reply,
        };
        let families: Vec<&str> = match wanted.split_once(':') {
            Some((family, _)) if checks.families().any(|(n, _)| n == family) => vec![family],
            _ => checks.families().map(|(name, _)| name).collect(),
        };
        let mut run = CheckRun::default();
        let prepared = checks.prepare(ctx, &tree, &families, &options, &mut run);
        let found: Vec<(&Prepared, &Keyed)> = prepared
            .iter()
            .flat_map(|p| p.keyed.iter().map(move |k| (p, k)))
            .filter(|(_, k)| matches(wanted, k.listed.check.as_str()))
            .collect();
        let exact = found
            .iter()
            .find(|(_, k)| k.listed.check.as_str() == *wanted);
        let (family, keyed) = match (exact, found.as_slice()) {
            (Some(one), _) | (None, [one]) => *one,
            (None, []) => return Reply::text(Exit::Usage, format!("no check matches {wanted}")),
            (None, many) => {
                let names: Vec<&str> = many
                    .iter()
                    .take(5)
                    .map(|(_, k)| k.listed.check.as_str())
                    .collect();
                return Reply::text(
                    Exit::Usage,
                    format!("{} checks match {wanted}: {}", many.len(), names.join(", ")),
                );
            }
        };
        let hit = checks.cache().lookup(std::slice::from_ref(&keyed.key));
        let hit = hit.into_iter().next().flatten();
        let lines = if flags.contains(&"--inputs") {
            input_lines(&tree, keyed, hit.as_ref())
        } else {
            let single = Prepared {
                family: family.family.clone(),
                keyed: vec![keyed.clone()],
                broken: Vec::new(),
            };
            let view = gate_view(
                &checks,
                ctx,
                &tree,
                std::slice::from_ref(&single),
                std::slice::from_ref(&hit),
            );
            record_lines(ctx, &checks, &view, keyed, &family.family, hit.as_ref())
        };
        Reply::text(Exit::Success, render(&lines))
    }
}

/// What the three verbs share: finding the tree and reading the gate's inputs.
trait TreeVerb {
    fn checks(&self) -> &Checks;

    /// The tree the request came from: the one the daemon found for it, or the one that holds
    /// its working directory.
    /// The tree is opened with its own manifests, and every command on it goes through the
    /// runner returned with it (see [`Checks::open`]).
    fn tree(&self, ctx: &Context, req: &Request) -> Result<(Checks, OpenTree), Reply> {
        let checks = self.checks();
        let found = ctx.tree.clone();
        let id: TreeId = found
            .or_else(|| checks.trees.tree_of(&req.cwd))
            .ok_or_else(|| {
                Reply::text(
                    Exit::Usage,
                    format!("{} is not in a build tree", req.cwd.display()),
                )
            })?;
        checks
            .open(ctx, &id)
            .map_err(|e| Reply::text(Exit::Usage, format!("cannot read tree {id}: {e}")))
    }
}

/// What the gate plan reads: the tree's changed paths and areas, and each family's record.
/// Where the changed paths cannot be read, the view has none, which suits the verbs that only
/// explain a check; the gate itself reads them first and fails when it cannot.
fn gate_view(
    checks: &Checks,
    ctx: &Context,
    tree: &OpenTree,
    prepared: &[Prepared],
    hits: &[Option<CachedResult>],
) -> GateView {
    let changed = changed_paths(checks, ctx, tree).unwrap_or_default();
    gate_view_of(checks, ctx, tree, prepared, hits, &changed)
}

/// The paths the tree changes since its branch left the integration branch.
pub(super) fn changed_paths(
    checks: &Checks,
    ctx: &Context,
    tree: &OpenTree,
) -> ignite_core::traits::Result<Vec<String>> {
    let base = format!(
        "{}/{}",
        ctx.config.repository.remote, ctx.config.repository.staging
    );
    checks.trees.changed(&tree.path, &base)
}

/// [`gate_view`] over `changed`, which the caller read.
pub(super) fn gate_view_of(
    checks: &Checks,
    ctx: &Context,
    tree: &OpenTree,
    prepared: &[Prepared],
    hits: &[Option<CachedResult>],
    changed: &[String],
) -> GateView {
    let areas = plan::areas(changed, &tree.graph);
    let families = prepared.iter().map(|p| p.family.as_str());
    let stats = super::sources::stats_of(checks.history.as_ref(), families, &areas);
    let manifest = &checks.settings.manifest;
    let family = |name: &str| manifest.families.get(name).map(|d| &d.family);
    let regressions = open_regressions(ctx);
    let mut view = GateView::new(prepared, hits, family, &stats, &regressions, changed, areas);
    view.unbuilt = unbuilt(ctx, tree, prepared.iter().filter_map(|p| family(&p.family)));
    // The rate of the gates of the last hour on this daemon, or the hub's for the day, and the
    // checks the change's last gate chose (see `spent`).
    let daily = plan::class_rates(ctx).land();
    let conn = checks.cache.conn();
    let memory = super::spent::GateMemory(&conn);
    view.rate = memory.rate(ctx.clock.now(), daily);
    view.paid = memory.paid(changed);
    view
}

/// The build products that `families` need, to be listed or to run, which `tree` has never
/// built.
pub(super) fn unbuilt<'a>(
    ctx: &Context,
    tree: &OpenTree,
    families: impl Iterator<Item = &'a Family>,
) -> BTreeSet<String> {
    let mut products = BTreeSet::new();
    for family in families {
        products.extend(family.needs.iter().cloned());
        products.extend(family.list_needs.iter().flatten().cloned());
    }
    products.retain(|product| !ctx.builds.has_built(&tree.id, product));
    products
}

/// The open regression ticket of each check the hub has one for, including a check the hub
/// grouped onto another check's ticket. A hub that cannot be reached answers none, which only
/// means the gate plans every check as if none were known to fail. A hub that does not know the
/// request is read by the titles of its regression tickets (`Regression: <check>`), which name
/// only the check each ticket was filed for.
pub(super) fn open_regressions(ctx: &Context) -> BTreeMap<CheckName, TicketId> {
    if let Ok(open) = ctx.hub.open_regressions() {
        return open.into_iter().map(|o| (o.check, o.ticket)).collect();
    }
    let query = TicketQuery {
        label: vec!["regression".to_owned()],
        limit: Some(1000),
        ..TicketQuery::default()
    };
    let tickets = ctx.hub.tickets(&query).unwrap_or_default();
    tickets
        .into_iter()
        .filter(|t| t.status != Status::Closed)
        .filter_map(|t| {
            let check = t.title.strip_prefix("Regression: ")?.parse().ok()?;
            Some((check, t.id))
        })
        .collect()
}

impl TreeVerb for TestVerb {
    fn checks(&self) -> &Checks {
        &self.checks
    }
}

impl TreeVerb for CheckVerb {
    fn checks(&self) -> &Checks {
        &self.checks
    }
}

impl TreeVerb for WhyVerb {
    fn checks(&self) -> &Checks {
        &self.checks
    }
}

fn options(verb: &str, req: &Request, rerun: bool) -> RunOptions {
    RunOptions {
        verb: verb.to_owned(),
        class: Class::Agent,
        rerun,
        ticket: None,
        session: req.session.clone(),
        reserve: None,
    }
}

/// The arguments that are not flags, and the flags.
fn split_args(args: &[String]) -> (Vec<&str>, Vec<&str>) {
    args.iter()
        .map(String::as_str)
        .partition(|a| !a.starts_with("--"))
}

/// Whether `name` is matched by `wanted`: as a pattern when it holds `*` or `?`, else as a
/// substring.
pub(crate) fn matches(wanted: &str, name: &str) -> bool {
    fn glob(pattern: &[u8], text: &[u8]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|i| glob(rest, &text[i..])),
            Some((b'?', rest)) => !text.is_empty() && glob(rest, &text[1..]),
            Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
        }
    }
    if !wanted.contains(['*', '?']) {
        return name.contains(wanted);
    }
    glob(wanted.as_bytes(), name.as_bytes())
}

/// Labelled lines in aligned columns.
pub(crate) fn render(lines: &[(String, String)]) -> String {
    let mut text = String::new();
    for (label, value) in lines {
        let line = format!("{label:<8} {value}");
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

fn reply(run: &CheckRun, text: String) -> Reply {
    let exit = if run.passed() {
        Exit::Success
    } else {
        Exit::CheckFailed
    };
    Reply {
        exit,
        text,
        json: Some(json!({
            "outcomes": run.outcomes.iter().map(|(c, o)| json!({"check": c, "outcome": o})).collect::<Vec<_>>(),
            "from_cache": run.from_cache,
            "ran": run.ran,
            "failures": run.failures.iter().map(|f| json!({
                "check": f.check, "outcome": f.outcome, "at": f.at,
                "message": f.message, "output": f.output,
            })).collect::<Vec<_>>(),
            "build_failures": run.build_failures,
            "incomplete_families": run.incomplete_families,
        })),
        handle: None,
    }
}

pub(super) fn counts(run: &CheckRun) -> String {
    let parts: Vec<String> = [
        (Outcome::Pass, "passed"),
        (Outcome::Fail, "failed"),
        (Outcome::Flaky, "flaky"),
        (Outcome::Incomplete, "incomplete"),
    ]
    .iter()
    .map(|(o, word)| (run.count(*o), word))
    .filter(|(n, _)| *n > 0)
    .map(|(n, word)| format!("{n} {word}"))
    .collect();
    if parts.is_empty() {
        "nothing ran".to_owned()
    } else {
        parts.join(", ")
    }
}

fn ran_line(run: &CheckRun) -> String {
    let verdict = if run.passed() && !run.outcomes.is_empty() {
        "all passed".to_owned()
    } else {
        counts(run)
    };
    let cache = if run.from_cache > 0 {
        format!(" ({} from cache)", run.from_cache)
    } else {
        String::new()
    };
    format!("{} checks: {verdict}{cache}", run.outcomes.len())
}

pub(super) fn failure_lines(run: &CheckRun) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for (product, output) in &run.build_failures {
        lines.push((
            "BUILD".to_owned(),
            format!("{product} failed; output in {}", output.display()),
        ));
    }
    for (family, output) in &run.incomplete_families {
        lines.push((
            "INCOMPLETE".to_owned(),
            format!("{family}: its list failed; output in {}", output.display()),
        ));
    }
    for failure in run.failures.iter().take(SHOWN_FAILURES) {
        let label = if failure.outcome == Outcome::Fail {
            "FAIL"
        } else {
            "INCOMPLETE"
        };
        lines.push((label.to_owned(), failure.check.to_string()));
        let detail: Vec<&str> = [failure.at.as_deref(), failure.message.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        if !detail.is_empty() {
            lines.push((String::new(), detail.join("  ")));
        }
    }
    if run.failures.len() > SHOWN_FAILURES {
        lines.push((
            "more".to_owned(),
            format!("{} more; see --json", run.failures.len() - SHOWN_FAILURES),
        ));
    }
    lines
}

/// `2026-11-17T14:12:00.000Z` as `2026-11-17 14:12`.
fn minute(result: &CachedResult) -> String {
    result
        .at
        .to_string()
        .replace('T', " ")
        .chars()
        .take(16)
        .collect()
}

fn cache_text(hit: Option<&CachedResult>) -> String {
    match hit {
        Some(hit) => {
            let verb = if hit.outcome == Outcome::Pass {
                "passed"
            } else {
                "failed"
            };
            format!("{verb} on {} at {}", hit.machine, minute(hit))
        }
        None => "no result for these inputs".to_owned(),
    }
}

fn record_lines(
    ctx: &Context,
    checks: &Checks,
    view: &GateView,
    keyed: &Keyed,
    family_name: &str,
    hit: Option<&CachedResult>,
) -> Vec<(String, String)> {
    let check = &keyed.listed.check;
    let stats = view.stats.get(check);
    let mut lines = why_record::head_lines(checks, check.as_str(), family_name, stats);
    lines.push(("cache".to_owned(), cache_text(hit)));
    let gate_plan = view.plan(&ctx.config.gate);
    let threshold = ctx.config.gate.threshold;
    let runs = if let Some(p) = gate_plan.run.iter().chain(&gate_plan.cached).next() {
        match p.score {
            None => "in every landing its inputs touch (the always set)".to_owned(),
            Some(score) => format!("in the gate: score {score:.4}, threshold {threshold}"),
        }
    } else if let Some(left) = gate_plan.past_budget.first() {
        format!("at the tip: score {:.4} is past the budget", left.score)
    } else if let Some(left) = gate_plan.cold.first() {
        format!(
            "at the tip: score {:.4}, but it needs a product this tree has never built",
            left.score
        )
    } else if let Some(left) = gate_plan.tip.first() {
        format!(
            "at the tip: score {:.4} is below the threshold {threshold}",
            left.score
        )
    } else {
        "nowhere: switched off".to_owned()
    };
    lines.push(("runs".to_owned(), runs));
    lines
}

fn input_lines(
    tree: &OpenTree,
    keyed: &Keyed,
    hit: Option<&CachedResult>,
) -> Vec<(String, String)> {
    let rows: Vec<(&str, String, &str, String)> = keyed
        .inputs
        .iter()
        .map(|(path, oid)| {
            let short: String = oid.as_str().chars().take(7).collect();
            if let Some(package) = tree.graph.packages.iter().find(|p| p.dir == *path) {
                ("crate", package.name.clone(), "tree", short)
            } else if tree.tracked.contains(path) {
                ("file", path.clone(), "blob", short)
            } else {
                ("dir", path.clone(), "tree", short)
            }
        })
        .collect();
    let width = rows.iter().map(|r| r.1.len()).max().unwrap_or(0) + 2;
    let crates = rows.iter().filter(|r| r.0 == "crate").count();
    let mut lines = Vec::new();
    let mut crates_shown = 0;
    for (kind, name, object, short) in &rows {
        if *kind == "crate" {
            crates_shown += 1;
            if crates_shown > 3 {
                continue;
            }
        }
        let mut value = format!("{name:<width$}{object} {short}");
        if *kind == "crate" && crates_shown == 3 && crates > 3 {
            let _ = write!(value, "   (+{} more crates)", crates - 3);
        }
        lines.push(((*kind).to_owned(), value));
    }
    let hash = keyed.key.hash.as_str();
    lines.push((
        "hash".to_owned(),
        format!(
            "{}…{}   {}",
            &hash[..4],
            &hash[hash.len() - 4..],
            cache_text(hit)
        ),
    ));
    lines
}
