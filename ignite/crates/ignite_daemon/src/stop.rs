//! `ignite stop <handle>`: ends a running command of the asking session's own.
//!
//! The command's work is asked to stop through its activity (see [`crate::steps`]). Its
//! supervised commands are ended the way those of a caller that left are, a command waiting for
//! cores withdraws its request, and nothing new starts. The verb then unwinds as from a failure,
//! which gives back its cores, resources, landing tree and lease, and its handle ends `stopped`
//! with exit code 3. A landing whose push has begun refuses to stop, with the reason, because a
//! push cut short may or may not have reached the remote.
//!
//! `ignite close` and `ignite release` stop the running commands of the tree they free in the
//! same way (see [`crate::trees`]).

use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply, Request};

use crate::handles::{Handles, Waited};
use crate::server::{Context, Verb, still_running_at};
use crate::steps::Steps;

/// How long `ignite stop` waits for the command to end. Its processes get `SIGTERM`, then
/// `SIGKILL` ten seconds later, and a request waiting for cores is withdrawn within a second, so
/// this is only reached by a verb that does not look at its stop between commands.
pub const STOP_WAIT: Duration = Duration::from_secs(60);

pub struct StopVerb {
    handles: Arc<Handles>,
    steps: Arc<Steps>,
    wait: Duration,
}

impl StopVerb {
    pub fn new(handles: Arc<Handles>, steps: Arc<Steps>) -> StopVerb {
        StopVerb {
            handles,
            steps,
            wait: STOP_WAIT,
        }
    }

    /// The verb with a shorter wait for the command to end, for tests.
    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> StopVerb {
        self.wait = wait;
        self
    }
}

impl Verb for StopVerb {
    fn name(&self) -> &'static str {
        "stop"
    }

    /// It waits at most [`STOP_WAIT`] by itself.
    fn keeps_its_own_wait(&self) -> bool {
        true
    }

    /// The session is checked against the command's own instead.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let [name] = req.args.as_slice() else {
            return Reply::text(Exit::Usage, "usage    ignite stop <handle>");
        };
        let Ok(handle) = Handle::new(name.as_str()) else {
            return Reply::text(Exit::Usage, format!("unknown handle: {name}"));
        };
        let Some(activity) = self.steps.by_handle(&handle) else {
            return match self.handles.wait(&handle, 0, None) {
                Waited::Unknown => Reply::text(Exit::Usage, format!("unknown handle: {name}")),
                _ => Reply::text(
                    Exit::Success,
                    format!(
                        "finished {handle} is not running; nothing to stop\nnext     ignite wait \
                         {handle} prints its reply"
                    ),
                ),
            };
        };
        let owner = activity.work().session;
        if owner.is_none() || owner != ctx.session {
            let holder = owner.map_or("no session".to_owned(), |s| format!("session {s}"));
            return Reply::text(
                Exit::Conflict,
                format!(
                    "conflict {handle} belongs to {holder}; a session stops only its own \
                     commands\nnext     ignite tell that session to stop it"
                ),
            );
        }
        if let Err(reason) = activity.request_stop() {
            return Reply::text(
                Exit::Conflict,
                format!(
                    "refused  {handle} cannot be stopped: {reason}\nnext     ignite wait {handle}"
                ),
            );
        }
        if !activity.wait_finished(self.wait) {
            let view = activity.view();
            let mut reply = still_running_at(&view.name, &handle, Some(&view));
            reply.text = format!(
                "stopping {handle}: its processes were told to end, and it has not finished \
                 within {} s\n{}",
                self.wait.as_secs(),
                reply.text
            );
            return reply;
        }
        let verb = match activity.kind() {
            crate::steps::Kind::Verb(verb) | crate::steps::Kind::Loop(verb) => verb.clone(),
        };
        Reply {
            exit: Exit::Success,
            text: format!(
                "stopped  `ignite {verb}` ({handle}); its processes were ended and its cores, \
                 trees and lease given back"
            ),
            json: Some(serde_json::json!({ "handle": handle, "verb": verb, "stopped": true })),
            handle: None,
        }
    }
}

#[cfg(test)]
#[path = "stop_tests.rs"]
mod tests;
