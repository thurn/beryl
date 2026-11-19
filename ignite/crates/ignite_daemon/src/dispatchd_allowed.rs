//! `ignite dispatch allowed <tool> [<session>]`: whether the instruction the dispatcher holds
//! calls for a session tool on a session.
//!
//! Without this, nothing but the dispatcher's instructions stopped it from starting, clearing or
//! messaging a session no instruction named. A weaker model, or one confused after its context
//! was compacted, could start duplicate workers or clear a busy one. `ignite pool init` writes a
//! Claude Code hook that runs before each such tool call and blocks it when this answers no.
//!
//! An instruction is current from when `ignite dispatch --wait` gives it until it is
//! acknowledged, including while it waits to be given again after going unacknowledged for two
//! minutes, since the dispatcher may still be carrying it out. What each tool needs:
//!
//! - `clear_session`: a current hand-off to that session with `clear` set.
//! - `set_session_model` and `set_session_effort`: a current hand-off to that session.
//! - `send_message`: a current message to that session, or a current hand-off to it whose model
//!   and effort the daemon has read back as asked (see `raw`), so a prompt never reaches a worker
//!   on the wrong model.
//! - `start_session`: a current hand-off that names no session (a Codex slot's new chat), a
//!   `rotate` given in the last ten minutes, the pool setup of `/ignite pool <n>` (from
//!   `ignite pool add --dispatcher` until the workers are registered, at most fifteen minutes),
//!   or no Claude dispatcher registered on the machine yet.

use std::fmt::Write as _;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{DispatchAction, Exit, Reply};
use ignite_core::protocol::hub::WorkerRole;
use ignite_core::time::Timestamp;

use super::{Dispatcher, Purpose, Queued, State};
use crate::server::Context;

/// Seconds after `ignite pool add --dispatcher` during which the dispatcher may start workers.
pub const SETUP_S: f64 = 900.0;
/// Seconds after a `rotate` during which the dispatcher may open its successor.
pub const ROTATE_S: f64 = 600.0;

/// The session tools the hook asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Start,
    Clear,
    Model,
    Effort,
    Message,
}

impl Tool {
    /// The tool from its name, with or without its MCP server's prefix.
    fn parse(name: &str) -> Option<Tool> {
        let bare = name
            .strip_prefix("mcp__ccd_session__")
            .or_else(|| name.strip_prefix("mcp__ccd_session_mgmt__"))
            .unwrap_or(name);
        match bare {
            "start_session" => Some(Tool::Start),
            "clear_session" => Some(Tool::Clear),
            "set_session_model" => Some(Tool::Model),
            "set_session_effort" => Some(Tool::Effort),
            "send_message" => Some(Tool::Message),
            _ => None,
        }
    }

    /// Whether `queued`, a current instruction, calls for this tool on `session`. `verified` is
    /// whether a hand-off's read-back matched.
    fn called_for(self, queued: &Queued, verified: bool, session: Option<&str>) -> bool {
        let body = &queued.body;
        let names = |s: Option<&str>| body.session.as_ref().map(SessionId::as_str) == s;
        let handoff = matches!(queued.purpose, Purpose::Handoff(_));
        match self {
            Tool::Start => handoff && body.session.is_none(),
            Tool::Clear => handoff && body.clear && session.is_some() && names(session),
            Tool::Model | Tool::Effort => handoff && session.is_some() && names(session),
            Tool::Message => {
                ((handoff && verified) || body.action == DispatchAction::Message)
                    && session.is_some()
                    && names(session)
            }
        }
    }
}

/// The instructions the dispatcher may be carrying out now, each with whether it is a hand-off
/// whose read-back matched.
fn current(state: &State) -> Vec<(&Queued, bool)> {
    state
        .given
        .values()
        .map(|given| (&given.queued, given.verified))
        .chain(
            state
                .pending
                .iter()
                .filter(|queued| queued.gives > 0)
                .map(|queued| (queued, false)),
        )
        .collect()
}

impl Dispatcher {
    /// Opens the pool setup's window, in which the dispatcher may start workers, when `open`
    /// (a dispatcher registered itself), and closes it otherwise (its workers are registered).
    pub fn pool_setup(&self, open: bool, now: Timestamp) {
        self.state().setup_since = open.then_some(now);
    }

    /// Answers `ignite dispatch allowed`: success when a current instruction calls for `tool` on
    /// `session`, and a refusal naming the current instructions otherwise.
    pub fn allowed(&self, ctx: &Context, tool: &str, session: Option<&str>) -> Reply {
        let Some(parsed) = Tool::parse(tool) else {
            return Reply::text(
                Exit::CheckFailed,
                format!("blocked  {tool}: the dispatcher's loop never calls it"),
            );
        };
        let now = ctx.clock.now();
        let no_dispatcher = parsed == Tool::Start
            && !self
                .pool
                .members()
                .unwrap_or_default()
                .iter()
                .any(|m| m.role == WorkerRole::Dispatcher && m.app == "claude");
        let state = self.state();
        let within = |since: Option<Timestamp>, limit: f64| {
            since.is_some_and(|at| now.seconds_since(at) < limit)
        };
        let current = current(&state);
        let allowed = current
            .iter()
            .any(|(q, verified)| parsed.called_for(q, *verified, session))
            || (parsed == Tool::Start
                && (no_dispatcher
                    || within(state.setup_since, SETUP_S)
                    || within(state.rotating_since, ROTATE_S)));
        if allowed {
            return Reply::text(Exit::Success, "allowed");
        }
        let target = session.map_or_else(String::new, |s| format!(" on {s}"));
        let mut text = format!(
            "blocked  {tool}{target}: no instruction from `ignite dispatch --wait` calls for it."
        );
        if current.is_empty() {
            text.push_str("\ncurrent  none");
        }
        for (queued, verified) in current {
            let line = serde_json::to_string(&queued.body).unwrap_or_default();
            let _ = write!(text, "\ncurrent  {line}");
            if parsed == Tool::Message
                && matches!(queued.purpose, Purpose::Handoff(_))
                && !verified
                && queued.body.session.as_ref().map(SessionId::as_str) == session
            {
                let _ = write!(
                    text,
                    "\nreason   the prompt waits for the read-back: call get_session and report \
                     `ignite dispatch ack {} raw '<its JSON>'`",
                    queued.body.id
                );
            }
        }
        text.push_str(
            "\nnext     carry out only the current instruction, report it with `ignite dispatch \
             ack`, then start the next wait as AGENTS.md says",
        );
        Reply::text(Exit::CheckFailed, text)
    }
}
