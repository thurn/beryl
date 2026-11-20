//! Raw tool results from a Claude dispatcher. Whether a session is running, idle or gone, and
//! whether a hand-off's model and effort stuck, are the two judgments where a mistake costs most:
//! a wrong `gone` releases a live worker's ticket, and a skipped read-back runs a ticket on the
//! wrong model. So the dispatcher does not make them. It reports what the app's `get_session`
//! returned, as `ignite dispatch ack <id> raw '<json or error>'`, and this module reads it.
//!
//! - For a `check`: an error saying the session was not found, or `isArchived: true`, is gone;
//!   `isRunning: true` is running; `isRunning: false` is idle. Anything else, including a
//!   result about another session, is unknown, which proves nothing.
//! - For a hand-off to a named session: the read-back's `model` and `effort` must be what the
//!   instruction asked for. If they are, the instruction stays open and the reply tells the
//!   dispatcher to send the prompt and then report `ok`; if not, the hand-off fails as if the
//!   dispatcher had reported `failed`, and the prompt is never sent. An `ok` for such a hand-off
//!   with no matching read-back is refused.
//!
//! Codex dispatchers keep answering a check with `running|idle|gone|unknown`, and their hand-offs
//! open new chats, which have nothing to read back.

use serde_json::Value;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply};

use super::{Dispatcher, Given, State, slots};
use crate::server::Context;

/// What a `check`'s raw `get_session` result says of `session`: `running`, `idle`, `gone` or
/// `unknown`.
pub(super) fn check_answer(session: &SessionId, raw: &str) -> &'static str {
    match serde_json::from_str::<Value>(raw.trim()) {
        Ok(Value::Object(fields)) => {
            if fields.get("sessionId").and_then(Value::as_str) != Some(session.as_str()) {
                return "unknown";
            }
            if fields.get("isArchived").and_then(Value::as_bool) == Some(true) {
                return "gone";
            }
            match fields.get("isRunning").and_then(Value::as_bool) {
                Some(true) => "running",
                Some(false) => "idle",
                None => "unknown",
            }
        }
        Ok(_) => "unknown",
        // The app's error for a session that does not exist is `Session <id> not found.` Any
        // other error, such as a timeout, is a failed lookup.
        Err(_) => {
            let lower = raw.to_lowercase();
            if lower.contains(&session.as_str().to_lowercase()) && lower.contains("not found") {
                "gone"
            } else {
                "unknown"
            }
        }
    }
}

/// Why a hand-off's raw read-back does not match what `body` asked for, or `None` when it does.
pub(super) fn readback_mismatch(body: &DispatchInstruction, raw: &str) -> Option<String> {
    let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(raw.trim()) else {
        return Some(format!(
            "the read-back is not get_session's JSON: {}",
            short(raw)
        ));
    };
    let field = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    if let Some(session) = &body.session
        && field("sessionId").as_deref() != Some(session.as_str())
    {
        return Some(format!(
            "the read-back is of {}, not {session}",
            field("sessionId").unwrap_or_else(|| "no session".to_owned())
        ));
    }
    if let Some(model) = &body.model {
        let got = field("model");
        if got.as_deref().map(base_model) != Some(base_model(model)) {
            return Some(format!(
                "model read back as {}, not {model}",
                got.unwrap_or_else(|| "nothing".to_owned())
            ));
        }
    }
    if let Some(effort) = &body.effort {
        let got = field("effort");
        if got.as_deref() != Some(effort.as_str()) {
            return Some(format!(
                "effort read back as {}, not {effort}",
                got.unwrap_or_else(|| "nothing".to_owned())
            ));
        }
    }
    None
}

/// A model ID without a context-window suffix such as `[1m]`, which picks the same model.
fn base_model(model: &str) -> &str {
    let model = model.trim();
    match model
        .strip_suffix(']')
        .and_then(|m| m.rfind('[').map(|at| &model[..at]))
    {
        Some(base) => base,
        None => model,
    }
}

pub(super) fn short(raw: &str) -> String {
    let raw = raw.trim();
    match raw.char_indices().nth(80) {
        Some((at, _)) => format!("{}...", &raw[..at]),
        None => raw.to_owned(),
    }
}

/// Whether the instruction is a hand-off to a named session, whose model and effort the daemon
/// checks from a read-back. A Codex slot's hand-off opens a new chat and names no session.
fn reads_back(body: &DispatchInstruction) -> bool {
    body.action == DispatchAction::Handoff
        && body.session.as_ref().is_some_and(|s| !slots::is_slot(s))
}

fn body_of(state: &State, id: u64) -> Option<DispatchInstruction> {
    state
        .given
        .get(&id)
        .map(|g| g.queued.body.clone())
        .or_else(|| {
            state
                .pending
                .iter()
                .find(|q| q.body.id == id)
                .map(|q| q.body.clone())
        })
}

impl Dispatcher {
    /// Acts on `ignite dispatch ack <id> raw <result>`.
    pub(super) fn ack_raw(&self, ctx: &Context, id: u64, raw: &str) -> Reply {
        let mut state = self.state();
        let Some(body) = body_of(&state, id) else {
            return Reply::text(
                Exit::Usage,
                format!("no instruction {id} is waiting for an acknowledgement"),
            );
        };
        if body.action == DispatchAction::Usage {
            drop(state);
            return self.ack_usage(ctx, id, raw);
        }
        if body.action == DispatchAction::Check
            && let Some(session) = &body.session
        {
            drop(state);
            let answer = check_answer(session, raw);
            let mut reply = self.ack(ctx, id, answer, "");
            reply.text = format!("read {session} as {answer}\n{}", reply.text);
            return reply;
        }
        if !reads_back(&body) {
            return Reply::text(
                Exit::Usage,
                format!(
                    "instruction {id} takes no raw result: only a check, a usage and a hand-off \
                     to a named session do"
                ),
            );
        }
        if body.clear
            && state.clears_reported.is_some()
            && !state.given.get(&id).is_some_and(|g| g.cleared)
        {
            drop(state);
            let mut reply = self.ack(
                ctx,
                id,
                "failed",
                "the worker was never reported cleared: the app refused the clear, or the clear \
                 was skipped",
            );
            reply.text.push_str("\nDo not send the prompt.");
            return reply;
        }
        if let Some(why) = readback_mismatch(&body, raw) {
            drop(state);
            let mut reply = self.ack(ctx, id, "failed", &why);
            reply.text.push_str("\nDo not send the prompt.");
            return reply;
        }
        let now = ctx.clock.now();
        if let Some(index) = state.pending.iter().position(|q| q.body.id == id)
            && let Some(mut queued) = state.pending.remove(index)
        {
            queued.gives += 1;
            state.given.insert(
                id,
                Given {
                    queued,
                    at: now,
                    verified: false,
                    cleared: false,
                },
            );
        }
        if let Some(given) = state.given.get_mut(&id) {
            given.verified = true;
            // The prompt still has to be sent: the two minutes for an acknowledgement start
            // again.
            given.at = now;
        }
        state.failures = 0;
        state.down = false;
        drop(state);
        self.save();
        Reply::text(
            Exit::Success,
            format!(
                "acked    {id} raw: model and effort read back as asked\nNow send `prompt` to \
                 the session word for word, then report `ignite dispatch ack {id} ok`."
            ),
        )
    }

    /// The refusal of an `ok` for a hand-off to a named session whose model and effort were not
    /// read back, or `None` when the `ok` may be recorded.
    pub(super) fn unread_handoff(&self, id: u64) -> Option<Reply> {
        let state = self.state();
        let verified = state.given.get(&id).is_some_and(|g| g.verified);
        let body = body_of(&state, id)?;
        (reads_back(&body) && !verified).then(|| {
            Reply::text(
                Exit::Usage,
                format!(
                    "hand-off {id} has no read-back: call get_session on {} and report \
                     `ignite dispatch ack {id} raw '<its JSON>'`, and send the prompt only when \
                     the reply says to",
                    body.session
                        .as_ref()
                        .map_or("the session", SessionId::as_str)
                ),
            )
        })
    }
}
