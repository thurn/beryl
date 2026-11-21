//! `streak`: mana or damage streams from one thing to another, as the web stage's `Streaks`
//! (`ui/src/stage/streaks.ts`) and Arena's mana and burn do. The step writes one `StreakRun`
//! over its own interval, whatever the amount it carries. Each frame, `light::streaks` draws it
//! toward the target's current position. The layout handles any slide from the new damage
//! record, for example when damage takes the card out of a pile.

use beryl_protocol::script::Act;

use super::{StepIn, StepOut};
use crate::light::StreakRun;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Streak {
        from,
        to,
        stream,
        amount,
        ..
    } = step.act
    else {
        return;
    };
    if step.own.duration() > 0 {
        out.streak(StreakRun {
            stream: *stream,
            from: *from,
            to: *to,
            amount: *amount,
            over: step.own,
            curve: step.curve,
            step: step.number,
        });
    }
}
