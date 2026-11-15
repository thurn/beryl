//! `count`: a number on the table changes as the step begins. A life total or a pool of unspent
//! mana is set then (`StepOut::count`); the `loss` or `gain` its step begins shows a total
//! falling or rising (`hud::counts`). A card's counters, damage or power and toughness are its
//! record's, which the step carries, and the card itself does not move.

use beryl_protocol::script::{Act, Tally};

use super::{StepIn, StepOut};
use crate::hud::counts::Count;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    match step.act {
        Act::Count {
            tally: Tally::Life { player, life },
        } => out.count(*player, Count::Life(*life)),
        Act::Count {
            tally: Tally::Pool { player, mana },
        } => out.count(*player, Count::Pool(*mana)),
        _ => out.instant(),
    }
}
