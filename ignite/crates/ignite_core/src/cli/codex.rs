//! Stub. The ticket "Codex CLI adapter: build a turn's command and read its events" owns this
//! file and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use super::{CliApp, TurnEnd, TurnEvents, TurnSpec};

/// The `codex` program, run as `codex exec --json`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Codex;

impl CliApp for Codex {
    fn name(&self) -> &'static str {
        "codex"
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
