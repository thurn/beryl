//! Matching casts by their number in the current turn.
//!
//! Implements: none. Filters the cast event after it has been counted.

use beryl_card::CastNumber;

use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Count, State};

/// Treats the cast as part of the count. See [CR 601.2i]: "Any abilities that trigger
/// when a spell is cast or put onto the stack trigger at this time."
///
/// [CR 601.2i]: https://yawgatog.com/resources/magic-rules/#R6012i
pub(super) fn matches<L: Layout>(s: &State<L>, caster: Seat, number: Option<CastNumber>) -> bool {
    match number {
        None => true,
        Some(CastNumber::InTurn(number)) => {
            s.players()
                .in_seat_order()
                .map(|player| u32::from(s.counted(Count::SpellsCast, player)))
                .sum::<u32>()
                == u32::from(number)
        }
        Some(CastNumber::ByCaster(number)) => s.counted(Count::SpellsCast, caster) == number,
    }
}
