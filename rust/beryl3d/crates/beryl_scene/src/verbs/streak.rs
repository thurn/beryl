//! `streak`: mana or damage streams from one thing to another, as the web stage's `Streaks`
//! (`ui/src/stage/streaks.ts`) and Arena's mana and burn do. The step writes one `StreakRun`
//! over its own interval, whatever the amount it carries, and `light::streaks` draws it while it
//! runs. Nothing moves: the card it lands on takes the record the act carries (its new damage,
//! say) as the step ends.

use beryl_protocol::script::Act;

use super::{StepIn, StepOut, instant};
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
    // The record the act carries, of what the streak lands on, is the table's from the step's
    // end. It changes no place unless the record does (a creature dealt lethal damage stays
    // where it is until it is destroyed by a step of its own).
    if let (Some(subject), Some(was), Some(rest)) = (step.subject, step.from, step.to)
        && was != rest
    {
        out.segment(subject, instant(was, rest, step.own.to));
    }
}
