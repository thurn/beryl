//! Beginning a turn while replacement effects can skip it.
//!
//! Implements: none. Runs a turn's beginning with replacement choices supplied by replay.

use crate::decision::Opt;
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::replacement::{self, event::Sink};
use crate::state::State;
use crate::table::GameTable;

use super::take_turn;

/// Applies each recorded replacement before beginning the first unskipped turn.
/// See [CR 614.10b]: "That action is considered to be the first thing that happens during the
/// next step, phase, or turn to actually occur." Keep the untaps in the order chosen and
/// perform them as the turn begins, before its untap step.
///
/// [CR 614.10b]: https://yawgatog.com/resources/magic-rules/#R61410b
pub(super) fn replay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let mut untaps = <L::Queue<_, 32>>::default();
    while let Some(choice) = replacement::turns::choose(s, t)? {
        let answer = replacement::answer(s, sink, &choice)?;
        let Some(Opt::Object(slot)) = choice.candidates.option(answer) else {
            break;
        };
        untaps.push(s.object_ref(slot))?;
        s.skip_turn();
    }
    take_turn(s, t, sink, untaps.as_slice())
}
