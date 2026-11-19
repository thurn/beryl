//! The verb process's connection to its daemon: the host protocol's calls over the daemon's
//! socket ([`SocketHost`]), and the retry that carries every call across a restart of the
//! daemon ([`Link`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Instant;

use ignite_core::host::HostClient;
use ignite_core::ids::Handle;
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::host::{
    Ack, AllowStop, Build, BuildReply, Finished, Hello, HelloReply, HostCall, LeaseLanding,
    LeaseLandingReply, Poll, RefuseStop, RefuseStopReply, ReleaseLanding, Run, RunReply,
    StepUpdate, StopState, decode_reply, encode_request,
};
use ignite_core::traits::{Error, Result};

use super::Timing;
use crate::handles::lock;
use crate::steps::Activity;

/// [`HostClient`] over the daemon's socket (a named pipe on Windows), one connection per call.
pub struct SocketHost {
    pub socket: String,
    /// The directory every call is sent from. It must not be inside a build tree: the daemon
    /// runs its hooks for a request from a leased tree, which counts as the session's activity.
    pub cwd: PathBuf,
}

impl SocketHost {
    fn call<C: HostCall>(&self, call: &C) -> Result<C::Reply> {
        let request = encode_request(call, self.cwd.clone());
        let reply = crate::server::send(&self.socket, &request)
            .map_err(|error| Error::Unreachable(format!("{}: {error}", C::VERB)))?;
        // A daemon that could not start answers waiting requests with the reason, and no body.
        if reply.exit == Exit::Unreachable && reply.json.is_none() {
            return Err(Error::Unreachable(format!("{}: {}", C::VERB, reply.text)));
        }
        decode_reply::<C>(reply)
    }
}

impl HostClient for SocketHost {
    fn hello(&self, call: &Hello) -> Result<HelloReply> {
        self.call(call)
    }
    fn run(&self, call: &Run) -> Result<RunReply> {
        self.call(call)
    }
    fn build(&self, call: &Build) -> Result<BuildReply> {
        self.call(call)
    }
    fn lease_landing(&self, call: &LeaseLanding) -> Result<LeaseLandingReply> {
        self.call(call)
    }
    fn release_landing(&self, call: &ReleaseLanding) -> Result<Ack> {
        self.call(call)
    }
    fn step(&self, call: &StepUpdate) -> Result<StopState> {
        self.call(call)
    }
    fn refuse_stop(&self, call: &RefuseStop) -> Result<RefuseStopReply> {
        self.call(call)
    }
    fn allow_stop(&self, call: &AllowStop) -> Result<Ack> {
        self.call(call)
    }
    fn poll(&self, call: &Poll) -> Result<StopState> {
        self.call(call)
    }
    fn finished(&self, call: &Finished) -> Result<Ack> {
        self.call(call)
    }
}

/// The host protocol as the verb process uses it. A call that fails because the daemon cannot
/// be reached, as while it restarts, is sent again with the same body until it is answered or
/// [`Timing::retry_for`] has passed; before each repeat the link sends `host-hello` and the
/// verb's current step, as the protocol asks. Every answer that says a stop was asked is passed
/// to the verb's activity.
pub struct Link {
    host: Arc<dyn HostClient>,
    hello: Hello,
    timing: Timing,
    /// The step last sent, or about to be: what a repeat after a failure sends again.
    current: Mutex<StepUpdate>,
    /// The start instant of the daemon that answered last. When an answer names another, the
    /// daemon restarted.
    daemon: Mutex<Option<String>>,
    /// The activity of the verb, once it runs, which hears about stops.
    activity: OnceLock<Weak<Activity>>,
}

impl Link {
    pub fn new(host: Arc<dyn HostClient>, hello: Hello, timing: Timing) -> Link {
        let current = StepUpdate {
            handle: hello.handle.clone(),
            step: None,
            waiting: Vec::new(),
            class: None,
            ticket: None,
        };
        Link {
            host,
            hello,
            timing,
            current: Mutex::new(current),
            daemon: Mutex::new(None),
            activity: OnceLock::new(),
        }
    }

    pub fn handle(&self) -> &Handle {
        &self.hello.handle
    }

    /// Passes every later stop the daemon reports to `activity`.
    pub fn attach(&self, activity: &Arc<Activity>) {
        let _ = self.activity.set(Arc::downgrade(activity));
    }

    fn notice(&self, stop: bool) {
        if stop && let Some(activity) = self.activity.get().and_then(Weak::upgrade) {
            let _ = activity.request_stop();
        }
    }

    /// Remembers which daemon answered. When it is another one than the one before, the daemon
    /// restarted, and the verb sends `host-hello` and its step at once: a verb that waits on a
    /// lock or in one long command makes no call that a restart would cut off, so nothing else
    /// would tell the new daemon about it. A failure is left to the next call's retry.
    fn note_daemon(&self, started: Option<&String>) {
        let Some(started) = started else {
            return;
        };
        let previous = lock(&self.daemon).replace(started.clone());
        if previous.is_some_and(|previous| previous != *started) {
            let current = lock(&self.current).clone();
            let _ = self.host.hello(&self.hello).and_then(|reply| {
                self.notice(reply.stop || !reply.running);
                self.host.step(&current)
            });
        }
    }

    /// Sends `send` until it is answered by a daemon or the retry window has passed.
    fn call<T>(&self, send: impl Fn(&dyn HostClient) -> Result<T>) -> Result<T> {
        let deadline = Instant::now() + self.timing.retry_for;
        loop {
            match send(self.host.as_ref()) {
                Err(Error::Unreachable(message)) => {
                    if Instant::now() >= deadline {
                        return Err(Error::Unreachable(message));
                    }
                    self.reintroduce(deadline)?;
                }
                answer => return answer,
            }
        }
    }

    /// After a failed call: waits a moment, then sends `host-hello` and the current step until
    /// both are answered.
    fn reintroduce(&self, deadline: Instant) -> Result<()> {
        loop {
            std::thread::sleep(self.timing.pause);
            let attempt = self.host.hello(&self.hello).and_then(|reply| {
                self.notice(reply.stop || !reply.running);
                lock(&self.daemon).clone_from(&reply.daemon_started);
                let current = lock(&self.current).clone();
                self.host.step(&current)
            });
            match attempt {
                Ok(state) => {
                    self.notice(state.stop);
                    return Ok(());
                }
                Err(Error::Unreachable(message)) if Instant::now() >= deadline => {
                    return Err(Error::Unreachable(message));
                }
                Err(Error::Unreachable(_)) => {}
                // The daemon answered; the call itself is sent again and gives its own answer.
                Err(_) => return Ok(()),
            }
        }
    }

    /// `host-hello`, sent until answered.
    pub fn hello(&self) -> Result<HelloReply> {
        let deadline = Instant::now() + self.timing.retry_for;
        loop {
            match self.host.hello(&self.hello) {
                Err(Error::Unreachable(message)) => {
                    if Instant::now() >= deadline {
                        return Err(Error::Unreachable(message));
                    }
                    std::thread::sleep(self.timing.pause);
                }
                Ok(reply) => {
                    self.notice(reply.stop || !reply.running);
                    lock(&self.daemon).clone_from(&reply.daemon_started);
                    return Ok(reply);
                }
                other => return other,
            }
        }
    }

    pub fn run(&self, call: &Run) -> Result<RunReply> {
        self.call(|host| host.run(call))
    }

    pub fn build(&self, call: &Build) -> Result<BuildReply> {
        self.call(|host| host.build(call))
    }

    pub fn lease_landing(&self, call: &LeaseLanding) -> Result<LeaseLandingReply> {
        self.call(|host| host.lease_landing(call))
    }

    pub fn release_landing(&self, call: &ReleaseLanding) -> Result<Ack> {
        self.call(|host| host.release_landing(call))
    }

    /// `host-step`, which also becomes the step sent again after a failed call.
    pub fn step(&self, update: &StepUpdate) -> Result<StopState> {
        update.clone_into(&mut lock(&self.current));
        let state = self.call(|host| host.step(update))?;
        self.notice(state.stop);
        self.note_daemon(state.daemon_started.as_ref());
        Ok(state)
    }

    pub fn poll(&self) -> Result<StopState> {
        let call = Poll {
            handle: self.handle().clone(),
        };
        let state = self.call(|host| host.poll(&call))?;
        self.notice(state.stop);
        self.note_daemon(state.daemon_started.as_ref());
        Ok(state)
    }

    pub fn refuse_stop(&self, reason: &str) -> Result<RefuseStopReply> {
        let call = RefuseStop {
            handle: self.handle().clone(),
            reason: reason.to_owned(),
        };
        self.call(|host| host.refuse_stop(&call))
    }

    pub fn allow_stop(&self) -> Result<Ack> {
        let call = AllowStop {
            handle: self.handle().clone(),
        };
        self.call(|host| host.allow_stop(&call))
    }

    pub fn finished(&self) -> Result<Ack> {
        let call = Finished {
            handle: self.handle().clone(),
        };
        self.call(|host| host.finished(&call))
    }
}
