//! Fast pre-pass of the landing gate: runs always-set families that need no compiled or unbuilt
//! product before starting heavy builds, so cheap failures (formatting, commit rules, source
//! rules, line-cap checks) refuse a check or landing in seconds instead of after minutes of
//! compilation.

use std::collections::BTreeSet;

use ignite_core::ids::CheckName;
use ignite_core::inputs;
use ignite_core::manifest::{Family, InputPattern, Product};
use ignite_core::protocol::hub::CachedResult;

use super::gate::{Excused, GateRun, gate_budget, landing_view, planned_families};
use super::plan;
use super::scope::GateChanges;
use super::time_lines::Stopwatch;
use super::verbs::gate_view_of;
use super::{CheckRun, Checks, OpenTree, Prepared, RunOptions};
use crate::server::Context;

/// Outcome of preparing all in-scope families, running the fast pre-pass first when present.
pub(super) enum PreparedGate {
    /// Listing and any fast pre-pass passed; ready to plan and run the remaining families.
    Ready {
        prepared: Vec<Prepared>,
        hits: Vec<Option<CachedResult>>,
        fast_done: BTreeSet<CheckName>,
        pre_running_s: f64,
    },
    /// A listing or fast pre-pass check failed; stop immediately.
    Failed(Box<GateRun>),
}

/// Inputs shared between the fast pre-pass and the rest of the gate.
pub(super) struct PrepassContext<'a> {
    pub changes: &'a GateChanges<'a>,
    pub catches: &'a plan::GateCatches,
    pub unbuilt: &'a BTreeSet<String>,
    pub excused: &'a Excused,
    pub scoped: &'a [String],
    pub carried: &'a [String],
}

type ListedGroup = (Vec<Prepared>, Vec<Option<CachedResult>>);

struct FastDone {
    prepared: Vec<Prepared>,
    hits: Vec<Option<CachedResult>>,
    done: BTreeSet<CheckName>,
    running_s: f64,
}

fn product_needs_work(product: &Product, changed: &[String]) -> bool {
    product.uses_build_slot()
        || product.inputs.iter().any(|input| match input {
            InputPattern::Path(pattern) => changed
                .iter()
                .any(|file| inputs::path_matches(pattern, file)),
            InputPattern::Crate(_) | InputPattern::CrateClosure => true,
        })
}

impl Checks {
    /// Whether `family` belongs to the fast pre-pass: it is in the always set for `changed`,
    /// does not compile (`builds`), needs no shared resource, and every product it needs is
    /// already built, non-compiled, and untouched by `changed`.
    pub(super) fn is_fast_prepass(
        &self,
        family: &Family,
        changed: &[String],
        unbuilt: &BTreeSet<String>,
    ) -> bool {
        family.always_for(changed)
            && !family.builds
            && family.resources.is_empty()
            && family
                .needs
                .iter()
                .chain(family.list_needs.iter().flatten())
                .all(|product| {
                    !unbuilt.contains(product)
                        && !self
                            .settings
                            .manifest
                            .products
                            .get(product)
                            .is_some_and(|p| product_needs_work(p, changed))
                })
    }

    /// Lists all `among` families, running any fast no-build always families first so cheap
    /// failures stop the gate before compiled builds start.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_with_prepass(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        cold: super::prepare::Cold<'_>,
        shared: &PrepassContext<'_>,
        run: &mut CheckRun,
    ) -> PreparedGate {
        let (fast, rest): (Vec<_>, Vec<_>) = among.iter().copied().partition(|(_, family)| {
            self.is_fast_prepass(family, shared.changes.paths, shared.unbuilt)
        });
        if fast.is_empty() || rest.is_empty() {
            return match self.list_group(ctx, tree, among, options, cold, shared, run) {
                Ok((prepared, hits)) => PreparedGate::Ready {
                    prepared,
                    hits,
                    fast_done: BTreeSet::new(),
                    pre_running_s: 0.0,
                },
                Err(failed) => PreparedGate::Failed(failed),
            };
        }
        let mut fast_done =
            match self.run_fast_prepass(ctx, tree, &fast, &rest, options, shared, run) {
                Ok(done) => done,
                Err(failed) => return PreparedGate::Failed(failed),
            };
        let (rest_prepared, rest_hits) =
            match self.list_group(ctx, tree, &rest, options, cold, shared, run) {
                Ok(listed) => listed,
                Err(failed) => return PreparedGate::Failed(failed),
            };
        fast_done.prepared.extend(rest_prepared);
        fast_done.hits.extend(rest_hits);
        PreparedGate::Ready {
            prepared: fast_done.prepared,
            hits: fast_done.hits,
            fast_done: fast_done.done,
            pre_running_s: fast_done.running_s,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn list_group(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[(&str, &Family)],
        options: &RunOptions,
        cold: super::prepare::Cold<'_>,
        shared: &PrepassContext<'_>,
        run: &mut CheckRun,
    ) -> Result<ListedGroup, Box<GateRun>> {
        let listing = Stopwatch::start(ctx);
        let mut prepared = self.prepare_gate(ctx, tree, families, options, run, cold);
        for family in &mut prepared {
            family.keep(|_| true, run);
        }
        run.stage(ctx, "listing", listing);
        if !run.passed() {
            return Err(Box::new(GateRun {
                run: std::mem::take(run),
                scoped: shared.scoped.to_vec(),
                carried: shared.carried.to_vec(),
                ..GateRun::default()
            }));
        }
        let hits = self.lookup(&prepared, options);
        Ok((prepared, hits))
    }

    /// Lists and runs `fast` before any family in `rest` is listed or built.
    #[allow(clippy::too_many_arguments)]
    fn run_fast_prepass(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        fast: &[(&str, &Family)],
        rest: &[(&str, &Family)],
        options: &RunOptions,
        shared: &PrepassContext<'_>,
        run: &mut CheckRun,
    ) -> Result<FastDone, Box<GateRun>> {
        let (fast_prepared, fast_hits) =
            match self.list_group(ctx, tree, fast, options, &|_, _| false, shared, run) {
                Ok(listed) => listed,
                Err(mut failed) => {
                    failed
                        .run
                        .unlisted
                        .extend(rest.iter().map(|(name, _)| (*name).to_owned()));
                    return Err(failed);
                }
            };
        let mut fast_view = gate_view_of(
            self,
            ctx,
            tree,
            &fast_prepared,
            &fast_hits,
            shared.changes,
            shared.catches.clone(),
        );
        landing_view(&mut fast_view, options, shared.excused);
        let fast_plan = fast_view.plan(&ctx.config.gate);
        let mut fast_families =
            planned_families(fast_prepared.clone(), fast_hits.clone(), &fast_plan);
        let fast_order = plan::run_order(&fast_view, &fast_plan);
        let fast_key = |family: &str| fast_order.get(family).copied().unwrap_or(0.0);
        fast_families.sort_by(|a, b| fast_key(&a.0).total_cmp(&fast_key(&b.0)));
        let pre_running = Stopwatch::start(ctx);
        self.run_families(ctx, tree, fast_families, options, run);
        let running_s = pre_running.seconds(ctx);
        run.stage(ctx, "running", pre_running);
        if !run.passed() {
            run.unlisted
                .extend(rest.iter().map(|(name, _)| (*name).to_owned()));
            return Err(Box::new(GateRun {
                run: std::mem::take(run),
                budget: Some(gate_budget(
                    &fast_view.budget(&ctx.config.gate),
                    &fast_plan,
                    shared.unbuilt.clone(),
                )),
                scoped: shared.scoped.to_vec(),
                carried: shared.carried.to_vec(),
                known_failing: fast_plan.known_failing,
                ..GateRun::default()
            }));
        }
        let done: BTreeSet<CheckName> = run.outcomes.iter().map(|(c, _)| c.clone()).collect();
        Ok(FastDone {
            prepared: fast_prepared,
            hits: fast_hits,
            done,
            running_s,
        })
    }
}
