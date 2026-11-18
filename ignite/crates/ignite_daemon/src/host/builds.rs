//! `host-build`: makes a product current in a tree with the daemon's build manager
//! (`Builds::ensure_for`), which joins a build already running for the same product.
//!
//! A new request ID gets a thread of its own that asks the build manager, inside the verb
//! process's activity, so the build is charged and stopped as one a verb on the daemon's own
//! thread starts. The call waits for it at most [`Timing::block`](super::Timing::block) and
//! answers `waiting` meanwhile; the next call with the same ID waits for the same thread. A call
//! repeated after the daemon restarted asks the build manager again, which joins the build
//! still running in its supervisor or finds the product current, so nothing is built twice.

use std::sync::Arc;
use std::time::Instant;

use ignite_core::protocol::host::{Build, BuildReply};
use ignite_core::traits::{BuildFor, Error, Result};

use super::{Host, Pending};
use crate::handles::lock;
use crate::server::Context;

impl Host {
    pub(super) fn build(self: &Arc<Self>, ctx: &Context, call: Build) -> Result<BuildReply> {
        if call.request_id.handle() != &call.handle {
            return Err(Error::Invalid(format!(
                "request {} is not one of {}",
                call.request_id, call.handle
            )));
        }
        let what = format!("building {} in {}", call.product, call.tree);
        let pending = self.pending_build(ctx, call)?;
        Ok(pending
            .wait(self.timing.block)
            .unwrap_or(BuildReply::Waiting { what }))
    }

    /// The thread working on `call`'s request ID: the one already started, or a new one.
    fn pending_build(
        self: &Arc<Self>,
        ctx: &Context,
        call: Build,
    ) -> Result<Arc<Pending<BuildReply>>> {
        let mut built = lock(&self.built);
        built.retain(|_, pending| !pending.stale());
        if let Some(pending) = built.get(&call.request_id) {
            return Ok(pending.clone());
        }
        let activity = self.running(ctx, &call.handle)?;
        let pending = Pending::new(Instant::now());
        let id = call.request_id.clone();
        built.insert(id.clone(), pending.clone());
        let builds = self.builds.clone();
        let settled = pending.clone();
        let thread = format!("ignite-host-build-{id}");
        let spawned = std::thread::Builder::new().name(thread).spawn(move || {
            let _entered = activity.enter();
            let who = BuildFor::from(call.who);
            let result = builds.ensure_for(&call.tree, &call.product, &call.packages, &who);
            settled.settle(BuildReply::from(result));
        });
        if let Err(error) = spawned {
            built.remove(&id);
            return Err(Error::Io(format!("cannot start a build thread: {error}")));
        }
        Ok(pending)
    }
}
