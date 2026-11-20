//! Stub. The ticket "Claude CLI adapter: build a turn's command and read its events" owns this
//! file and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use super::{CliApp, TurnEnd, TurnEvents, TurnSpec};

/// The `claude` program, run as `claude -p` with `--output-format stream-json --verbose`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Claude;

impl CliApp for Claude {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn command(&self, spec: &TurnSpec) -> Vec<String> {
        todo!()
    }

    fn read(&self, output: &str) -> TurnEvents {
        todo!()
    }

    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd {
        todo!()
    }
}
