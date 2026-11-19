//! The daemon verbs that answer the host protocol's calls, one per call.

use std::marker::PhantomData;
use std::sync::Arc;

use ignite_core::protocol::client::{Reply, Request};
use ignite_core::protocol::host::{
    Ack, AllowStop, BorrowCores, Build, CALLS, Finished, Hello, HostCall, LeaseLanding, Poll,
    RefuseStop, ReleaseLanding, ReturnCores, Run, Selected, StepUpdate, decode_request,
    encode_reply,
};
use ignite_core::traits::Result;

use super::Host;
use crate::server::{Context, Registry, Verb};

/// How the host answers one call.
type Answer<C> = fn(&Arc<Host>, &Context, C) -> Result<<C as HostCall>::Reply>;

/// The verb for the call `C`.
struct CallVerb<C: HostCall> {
    host: Arc<Host>,
    answer: Answer<C>,
    call: PhantomData<fn() -> C>,
}

impl<C: HostCall> Verb for CallVerb<C> {
    fn name(&self) -> &str {
        C::VERB
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let answer = decode_request::<C>(req).and_then(|call| (self.answer)(&self.host, ctx, call));
        encode_reply(&answer)
    }

    /// A call answers within `MAX_BLOCK_S` on the connection's thread and gets no handle: the
    /// verb process repeats it instead.
    fn keeps_its_own_wait(&self) -> bool {
        true
    }

    /// A verb process asks from the tree its verb works in, which is leased to a session; it
    /// sends no session of its own.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    /// A restart never waits for a call: the verb process repeats it to the new daemon.
    fn blocks_update(&self) -> bool {
        false
    }
}

fn add<C: HostCall + 'static>(host: &Arc<Host>, registry: &mut Registry, answer: Answer<C>) {
    registry.register(Box::new(CallVerb {
        host: Arc::clone(host),
        answer,
        call: PhantomData,
    }));
}

pub(super) fn register(host: &Arc<Host>, registry: &mut Registry) {
    add::<Hello>(host, registry, |host, ctx, call| Ok(host.hello(ctx, call)));
    add::<Run>(host, registry, |host, ctx, call| host.run(ctx, &call));
    add::<Build>(host, registry, Host::build);
    add::<LeaseLanding>(host, registry, |host, ctx, call| {
        host.lease_landing(ctx, &call)
    });
    add::<ReleaseLanding>(host, registry, |host, _, call| {
        host.release_landing(&call).map(|()| Ack {})
    });
    add::<StepUpdate>(host, registry, |host, ctx, call| Ok(host.step(ctx, &call)));
    add::<RefuseStop>(host, registry, |host, ctx, call| {
        host.refuse_stop(ctx, &call)
    });
    add::<AllowStop>(host, registry, |host, ctx, call| {
        host.allow_stop(ctx, &call.handle).map(|()| Ack {})
    });
    add::<Poll>(host, registry, |host, ctx, call| {
        Ok(host.poll(ctx, &call.handle))
    });
    add::<Finished>(host, registry, |host, _, call| {
        host.finished(&call.handle).map(|()| Ack {})
    });
    add::<BorrowCores>(host, registry, |host, _, call| Ok(host.borrow_cores(&call)));
    add::<ReturnCores>(host, registry, |host, _, call| {
        host.return_cores(&call.handle, call.grant);
        Ok(Ack {})
    });
    add::<Selected>(host, registry, |host, _, call| Ok(host.selected(&call)));
    debug_assert!(
        CALLS.iter().all(|(verb, _)| registry.get(verb).is_some()),
        "every call of the host protocol has a verb"
    );
}
