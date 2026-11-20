//! Verdicts: the record that one exact state of a change passed the landing gate with nothing
//! incomplete (see "Verdicts" in `docs/ignite.md`).
//!
//! A verdict is keyed by the git tree ID of every file in the ticket tree (committed or not),
//! the merge base of the tree's `HEAD` with the integration branch, and a digest of the
//! `[gate]` section. Nothing about the daemon's build or version is in the key: Ignite is
//! released many times an hour, and a verdict keyed by the release would be thrown away before
//! nearly every landing.
//!
//! Each verdict is kept at the hub, which every machine shares, and in the daemon's local
//! database, so that a landing still finds it while the hub cannot be reached.

use std::path::Path;

use rusqlite::Connection;

use ignite_core::ids::Commit;
use ignite_core::protocol::hub::{Verdict, VerdictKey};
use ignite_core::traits::{Error, Result};
use ignite_core::verdict_settings;
use ignite_store::local::verdicts as local;

use crate::server::Context;

/// The key of the tree at `tree_path` as it is on disk, whose branch is at `head`: its tree ID,
/// the merge base of `head` with the integration branch as this checkout last fetched it, and
/// the digest of the daemon's `[gate]` section, which is the section the gate runs under.
pub fn key_for(ctx: &Context, tree_path: &Path, head: &Commit) -> Result<VerdictKey> {
    let tree = ctx.git.tree_id(tree_path)?;
    let tip = ctx.git.known_tip(&ctx.config.repository.staging)?;
    let base = ctx.git.merge_base(tree_path, head, &tip)?;
    Ok(VerdictKey {
        tree,
        base,
        settings: verdict_settings(&ctx.config),
    })
}

/// The verdict stored under `key`: the hub's when it has one, otherwise the one in the local
/// database `db`. A hub that cannot be reached, or that does not know the route, answers
/// nothing and is not an error.
pub fn find(ctx: &Context, db: &Connection, key: &VerdictKey) -> Option<Verdict> {
    if let Ok(Some(verdict)) = ctx.hub.verdict(key)
        && verdict.key == *key
    {
        return Some(verdict);
    }
    let text = local::get(db, &key.tree, key.base.as_str(), &key.settings).ok()??;
    let verdict: Verdict = serde_json::from_str(&text).ok()?;
    (verdict.key == *key).then_some(verdict)
}

/// Stores `verdict` at the hub and in the local database `db`. It is an error only when
/// neither place took it; the error then names both reasons.
pub fn remember(ctx: &Context, db: &Connection, verdict: &Verdict) -> Result<()> {
    let at_hub = ctx.hub.store_verdict(verdict);
    let here = serde_json::to_string(verdict)
        .map_err(|e| Error::Invalid(e.to_string()))
        .and_then(|text| {
            let key = &verdict.key;
            local::put(
                db,
                &key.tree,
                key.base.as_str(),
                &key.settings,
                &text,
                verdict.passed_at,
                ctx.clock.now(),
            )
            .map_err(|e| Error::Io(e.to_string()))
        });
    match (at_hub, here) {
        (Err(hub), Err(local)) => Err(Error::Io(format!(
            "the hub did not take it ({hub}) and nor did the local database ({local})"
        ))),
        _ => Ok(()),
    }
}
