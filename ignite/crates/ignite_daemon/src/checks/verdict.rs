//! What `ignite check` stores when its gate passes: a verdict for the tree as it is (see
//! [`crate::verdicts`]).

use ignite_core::protocol::hub::Verdict;

use super::gate::GateRun;
use super::verbs::changed_paths;
use super::{Checks, OpenTree};
use crate::server::Context;
use crate::verdicts;

/// Stores a verdict for `tree` when `gated` passed with nothing incomplete, and returns the
/// reply line that says whether it was stored. A gate that did not pass stores nothing and
/// adds no line.
pub(super) fn store_lines(
    ctx: &Context,
    checks: &Checks,
    tree: &OpenTree,
    gated: &GateRun,
) -> Vec<(String, String)> {
    if !gated.run.passed() {
        return Vec::new();
    }
    let line = match store(ctx, checks, tree, gated) {
        Ok(()) => "stored for this tree".to_owned(),
        Err(error) => format!("not stored: {error}"),
    };
    vec![("verdict".to_owned(), line)]
}

fn store(
    ctx: &Context,
    checks: &Checks,
    tree: &OpenTree,
    gated: &GateRun,
) -> ignite_core::traits::Result<()> {
    let head = checks.trees.head(&tree.path)?;
    let key = verdicts::key_for(ctx, &tree.path, &head)?;
    let verdict = Verdict {
        key,
        head,
        machine: ctx.machine.as_str().to_owned(),
        passed_at: ctx.clock.now(),
        families: gated.scoped.clone(),
        left_to_tip: u32::try_from(gated.left).unwrap_or(u32::MAX),
        always_not_run: always_not_run(ctx, checks, tree, gated),
    };
    verdicts::remember(ctx, &checks.cache.conn(), &verdict)
}

/// The families in the gate's scope that are in the always set for this change but gave no
/// check an outcome, such as a family no machine here can run.
fn always_not_run(ctx: &Context, checks: &Checks, tree: &OpenTree, gated: &GateRun) -> Vec<String> {
    let changed = changed_paths(checks, ctx, tree).unwrap_or_default();
    let families = &checks.settings.manifest.families;
    gated
        .scoped
        .iter()
        .filter(|name| {
            families
                .get(name.as_str())
                .is_some_and(|declared| declared.family.always_for(&changed))
        })
        .filter(|name| {
            !gated
                .run
                .outcomes
                .iter()
                .any(|(check, _)| check.family() == name.as_str())
        })
        .cloned()
        .collect()
}
