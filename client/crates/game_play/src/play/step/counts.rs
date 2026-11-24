//! `StepOut::count`: what the `count` verb writes for a life total or a pool of unspent mana.

use beryl_protocol::protocol::Side;

use super::StepOut;
use crate::hud::counts::{Count, CountChange};

impl StepOut<'_> {
    /// A player's life total or unspent mana is set, as the step begins.
    pub fn count(&mut self, player: Side, count: Count) {
        self.written.counts.push(CountChange {
            player,
            count,
            at: self.own.from,
            step: self.number,
        });
    }
}
