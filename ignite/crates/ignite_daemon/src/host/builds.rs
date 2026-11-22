//! `host-build`: makes a product current in a tree with the daemon's build manager
//! (`Builds::ensure_for`), which joins a build already running for the same product.
//! `host-selected` asks the same manager which packages that build would select
//! (`Builds::selected`) and never waits for a build.
//!
//! A new request ID gets a thread of its own that asks the build manager, inside the verb
//! process's activity, so the build is charged and stopped as one a verb on the daemon's own
//! thread starts. The call waits for it at most [`Timing::block`](super::Timing::block) and
//! answers `waiting` meanwhile; the next call with the same ID waits for the same thread. A call
//! repeated after the daemon restarted asks the build manager again, which joins the build
//! still running in its supervisor or finds the product current, so nothing is built twice.

use std::sync::Arc;
use std::time::Instant;

use ignite_core::protocol::host::{
    Build, BuildReply, ExpressBuild, RequestId, Selected, SelectedReply,
};
use ignite_core::traits::{BuildFor, Builds, Error, Result};

use super::{Host, Pending};
use crate::handles::lock;
use crate::server::Context;

impl Host {
    pub(super) fn build(self: &Arc<Self>, ctx: &Context, call: Build) -> Result<BuildReply> {
        let handle = call.handle.clone();
        let id = call.request_id.clone();
        let what = format!("building {} in {}", call.product, call.tree);
        self.build_call(ctx, &handle, &id, what, move |builds| {
            let who = BuildFor::from(call.who);
            Ok(builds
                .ensure_for(&call.tree, &call.product, &call.packages, &who)
                .into())
        })
    }

    pub(super) fn express_build(
        self: &Arc<Self>,
        ctx: &Context,
        call: ExpressBuild,
    ) -> Result<BuildReply> {
        let handle = call.handle.clone();
        let id = call.request_id.clone();
        self.build_call(
            ctx,
            &handle,
            &id,
            "preparing express build outputs".to_owned(),
            move |builds| {
                builds.express_build(&call.tree, &call.output, call.stage)?;
                Ok(BuildReply::Current)
            },
        )
    }

    /// `host-selected`: what the build manager selects for the packages asked
    /// ([`Builds::selected`](ignite_core::traits::Builds::selected)).
    pub(super) fn selected(&self, call: &Selected) -> SelectedReply {
        SelectedReply {
            packages: self
                .builds
                .selected(&call.tree, &call.product, &call.packages),
        }
    }

    /// A request ID belongs to one operation whether the caller retries or the reply is delayed.
    fn build_call(
        self: &Arc<Self>,
        ctx: &Context,
        handle: &ignite_core::ids::Handle,
        id: &RequestId,
        what: String,
        work: impl FnOnce(&dyn Builds) -> Result<BuildReply> + Send + 'static,
    ) -> Result<BuildReply> {
        if id.handle() != handle {
            return Err(Error::Invalid(format!(
                "request {id} is not one of {handle}"
            )));
        }
        let pending = self.pending_build(ctx, handle, id, work)?;
        pending
            .wait(self.timing.block)
            .unwrap_or(Ok(BuildReply::Waiting { what }))
    }

    fn pending_build(
        self: &Arc<Self>,
        ctx: &Context,
        handle: &ignite_core::ids::Handle,
        id: &RequestId,
        work: impl FnOnce(&dyn Builds) -> Result<BuildReply> + Send + 'static,
    ) -> Result<Arc<Pending<Result<BuildReply>>>> {
        let mut built = lock(&self.built);
        built.retain(|_, pending| !pending.stale());
        if let Some(pending) = built.get(id) {
            return Ok(pending.clone());
        }
        let activity = self.running(ctx, handle)?;
        let pending = Pending::new(Instant::now());
        built.insert(id.clone(), pending.clone());
        let builds = self.builds.clone();
        let settled = pending.clone();
        let thread = format!("ignite-host-build-{id}");
        let spawned = std::thread::Builder::new().name(thread).spawn(move || {
            let _entered = activity.enter();
            settled.settle(work(builds.as_ref()));
        });
        if let Err(error) = spawned {
            built.remove(id);
            return Err(Error::Io(format!("cannot start a build thread: {error}")));
        }
        Ok(pending)
    }
}
