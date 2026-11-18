//! The `ignite dispatch` verb: the dispatcher's blocking wait and its acknowledgements.

use std::sync::Arc;
use std::time::Duration;

use ignite_core::protocol::client::{DispatchAction, DispatchInstruction, Exit, Reply, Request};

use super::{Dispatcher, FEED_WAIT_S, LOOP};
use crate::server::{Context, Verb};

/// `ignite dispatch [--wait <seconds>]` and `ignite dispatch ack <id> <result> [why]`.
pub struct DispatchVerb {
    dispatcher: Arc<Dispatcher>,
}

impl DispatchVerb {
    pub fn new(dispatcher: Arc<Dispatcher>) -> DispatchVerb {
        DispatchVerb { dispatcher }
    }
}

impl Verb for DispatchVerb {
    fn name(&self) -> &'static str {
        "dispatch"
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
        let wait = match args.as_slice() {
            ["--wait", seconds] => match seconds.parse::<u32>() {
                Ok(seconds) => seconds,
                Err(_) => return Reply::text(Exit::Usage, format!("not seconds: {seconds}")),
            },
            [] => req.wait_s,
            _ => {
                return Reply::text(
                    Exit::Usage,
                    "usage: ignite dispatch [--wait <seconds>] | ignite dispatch ack <id> <result>",
                );
            }
        };
        let deadline = ctx.clock.now().plus_seconds(f64::from(wait));
        loop {
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
