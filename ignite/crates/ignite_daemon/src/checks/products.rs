//! Making the build products a family's checks need current, and recording each build (see
//! "Checks" in `docs/ignite.md`).

use std::path::PathBuf;

use ignite_core::manifest::Family;
use ignite_core::protocol::Usage;
use ignite_core::protocol::hub::CompileUnit;
use ignite_core::traits::{BuildFor, BuildResult};

use super::{Built, CheckRun, Checks, OpenTree, RunOptions};
use crate::server::Context;

impl Checks {
    /// Makes each product the family needs current. `Err` holds a failed build's output.
    pub(super) fn build(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        packages: &[String],
        who: &BuildFor,
        run: &mut CheckRun,
    ) -> std::result::Result<(), PathBuf> {
        self.build_products(ctx, tree, &family.needs, packages, who, run)
    }

    /// Makes each of `products` current, in the class of the command `who` names and charged
    /// to its ticket and session. Each product built is sent to the hub as a record of verb
    /// `build` with its compile units, so which crates a build recompiled can be read later.
    /// `Err` holds a failed build's output.
    ///
    /// Families on threads of their own may ask for one product at once, and the build manager
    /// answers each with the one build's result; only the first reports and records it. A
    /// request that took a while without a build of its own is named on the `slow` line.
    pub(super) fn build_products(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        products: &[String],
        packages: &[String],
        who: &BuildFor,
        run: &mut CheckRun,
    ) -> std::result::Result<(), PathBuf> {
        for product in products {
            let asked = super::time_lines::Stopwatch::start(ctx);
            let result = ctx.builds.ensure_for(&tree.id, product, packages, who);
            let shared = match &result {
                BuildResult::Built { usage, .. } => {
                    let key = (
                        product.clone(),
                        usage.wall_s.to_bits(),
                        usage.cpu_s.to_bits(),
                    );
                    !self
                        .builds_seen
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(key)
                }
                _ => false,
            };
            match result {
                // Hashing the product's inputs, or waiting for another build that writes its
                // outputs, which then left it current.
                BuildResult::Current => {
                    run.slow(ctx, format!("finding {product} current"), asked);
                }
                BuildResult::Built { .. } if shared => {
                    run.slow(ctx, format!("waiting for the build of {product}"), asked);
                }
                BuildResult::Built { usage, units } => {
                    run.builds.push(Built {
                        product: product.clone(),
                        wall_s: usage.wall_s,
                        rebuilt: (!units.is_empty() || usage.cpu_s < 1.0)
                            .then(|| crate::build::recompiled(&units)),
                        costliest: crate::build::costliest(&units, super::time_lines::COSTLIEST),
                        cpu_s: usage.cpu_s,
                        queued_s: usage.queued_s,
                    });
                    self.record_build(ctx, tree, product, who, &usage, units);
                }
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

    /// Sends the hub a record of one product's build, verb `build` and the product as its
    /// family, holding the build's compile units.
    fn record_build(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        product: &str,
        who: &BuildFor,
        usage: &Usage,
        compile_units: Vec<CompileUnit>,
    ) {
        let options = RunOptions {
            verb: "build".to_owned(),
            class: who.class,
            rerun: false,
            ticket: who.ticket.clone(),
            session: who.session.clone(),
            reserve: None,
        };
        self.record(
            ctx,
            tree,
            product,
            &options,
            usage,
            "build",
            None,
            compile_units,
        );
    }
}
