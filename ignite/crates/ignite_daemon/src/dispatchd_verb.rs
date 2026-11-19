//! The `ignite dispatch` verb: the dispatcher's blocking wait and its acknowledgements.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply, Request};

use super::{Dispatcher, FEED_WAIT_S, LOOP};
use crate::server::{Context, Verb};

/// `ignite dispatch [--wait <seconds>]` and `ignite dispatch ack <id> <result> [why]`.
pub struct DispatchVerb {
    dispatcher: Arc<Dispatcher>,
    /// The latest wait each dispatcher session started, numbered in the order they started.
    waits: Mutex<Waits>,
}

#[derive(Default)]
struct Waits {
    started: u64,
    latest: BTreeMap<SessionId, u64>,
}

impl DispatchVerb {
    pub fn new(dispatcher: Arc<Dispatcher>) -> DispatchVerb {
        DispatchVerb {
            dispatcher,
            waits: Mutex::new(Waits::default()),
        }
    }

    /// Records a new wait by `session` and returns its number. It replaces any earlier wait by
    /// the same session, which then ends at its next turn of the loop (see [`Self::replaced`]).
    fn begin_wait(&self, session: &SessionId) -> u64 {
        let mut waits = self.waits.lock().unwrap_or_else(PoisonError::into_inner);
        waits.started += 1;
        let number = waits.started;
        waits.latest.insert(session.clone(), number);
        number
    }

    /// True when `session` has started a wait since wait `number`.
    fn replaced(&self, session: &SessionId, number: u64) -> bool {
        let waits = self.waits.lock().unwrap_or_else(PoisonError::into_inner);
        waits
            .latest
            .get(session)
            .is_some_and(|latest| *latest != number)
    }
}

/// Seconds before the request's own wait ends at which `dispatch` gives up waiting and
/// answers idle. The server hands the verb to a handle when the request's wait ends, and the
/// dispatcher loop would then have no caller to give an instruction to, so the verb has to
/// finish first, with room for the hub request it may be in the middle of.
pub const CALLER_MARGIN_S: u32 = 5;

/// How long `dispatch` may wait, in seconds: what `--wait` asked for, but no longer than the
/// request's own wait less [`CALLER_MARGIN_S`]. A request with no wait (`wait_s` of 0), such as
/// one run directly rather than through the server, is not limited.
fn waiting_limit_s(asked: u32, request_wait_s: u32) -> u32 {
    if request_wait_s == 0 {
        asked
    } else {
        asked.min(request_wait_s.saturating_sub(CALLER_MARGIN_S))
    }
}

impl Verb for DispatchVerb {
    fn name(&self) -> &'static str {
        "dispatch"
    }

    /// The dispatcher's loop is always inside `dispatch --wait`.
    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let args: Vec<&str> = req.args.iter().map(String::as_str).collect();
        if let ["ack", id, result, reason @ ..] = args.as_slice() {
            let Ok(id) = id.parse::<u64>() else {
                return Reply::text(Exit::Usage, format!("not an instruction id: {id}"));
            };
            let result = if *result == "fail" { "failed" } else { result };
            return self.dispatcher.ack(ctx, id, result, &reason.join(" "));
        }
        match args.as_slice() {
            ["allowed", tool] => return self.dispatcher.allowed(ctx, tool, None),
            ["allowed", tool, session] => return self.dispatcher.allowed(ctx, tool, Some(session)),
            ["cleared", session] => return self.dispatcher.cleared(ctx, session),
            _ => {}
        }
        let wait = match args.as_slice() {
            ["--wait", seconds] => match seconds.parse::<u32>() {
                Ok(seconds) => seconds,
                Err(_) => return Reply::text(Exit::Usage, format!("not seconds: {seconds}")),
            },
            [] => req.wait_s,
            _ => {
                return Reply::text(
                    Exit::Usage,
                    "usage: ignite dispatch [--wait <seconds>] | ignite dispatch ack <id> <result> \
                     | ignite dispatch allowed <tool> [<session>] | ignite dispatch cleared \
                     <session>",
                );
            }
        };
        // The dispatcher runs its wait in the background. Anything that wakes it meanwhile, such
        // as a notice from a worker, may lead it to start a second wait while the first still
        // runs, and two waits would each take instructions. The newer wait wins, so the
        // dispatcher never loses its wait by starting one too many.
        let number = req.session.as_ref().map(|session| self.begin_wait(session));
        let started = ctx.clock.now();
        let deadline = started.plus_seconds(f64::from(waiting_limit_s(wait, req.wait_s)));
        // After this the server has handed the verb to a handle and nobody is listening: an
        // instruction given now would be lost until it is given again two minutes later.
        let caller_leaves = (req.wait_s > 0).then(|| started.plus_seconds(f64::from(req.wait_s)));
        loop {
            if caller_leaves.is_some_and(|at| ctx.clock.now() >= at) {
                return render(&idle());
            }
            if let (Some(session), Some(number)) = (req.session.as_ref(), number)
                && self.replaced(session, number)
            {
                return Reply::text(Exit::Success, REPLACED);
            }
            if let Some(instruction) = self.dispatcher.step(ctx, req.session.as_ref()) {
                return render(&instruction);
            }
            let left = deadline.seconds_since(ctx.clock.now());
            if left <= 0.0 {
                return render(&idle());
            }
            // `left` is at most the u32 the wait was given in.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let wait_s = (left.ceil() as u32).min(FEED_WAIT_S);
            if self.dispatcher.poll_feed(ctx, wait_s).is_err() {
                // The hub is unreachable: wait out a short while rather than spin.
                std::thread::sleep(Duration::from_secs(u64::from(wait_s.min(10))));
            }
        }
    }
}

/// What a wait prints when a newer wait by the same dispatcher has replaced it.
pub const REPLACED: &str = "replaced  a newer `ignite dispatch --wait` from this session is \
                            waiting. Do nothing and end your turn.";

fn idle() -> DispatchInstruction {
    DispatchInstruction {
        id: 0,
        action: DispatchAction::Idle,
        session: None,
        clear: false,
        model: None,
        effort: None,
        prompt: None,
    }
}

/// The instruction as one JSON line, then the line that restates the loop.
fn render(instruction: &DispatchInstruction) -> Reply {
    let line = serde_json::to_string(instruction).unwrap_or_default();
    Reply {
        exit: Exit::Success,
        text: format!("{line}\n{LOOP}"),
        json: serde_json::to_value(instruction).ok(),
        handle: None,
    }
}
