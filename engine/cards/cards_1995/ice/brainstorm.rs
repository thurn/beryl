// Generated from the card store by `just card header`: notes go below the end marker.
//! Brainstorm {U}
//! Instant
//!
//! Draw three cards, then put two cards from your hand on top of your library in any order.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Brainstorm
    .abilities(&[Ability::spell().effect(&sequence(&[draw(you(), 3), put_back(you(), 2)]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new(
        "Brainstorm in a shared game puts two cards on top that the opponent draws",
    )
    .shared_library(&[Card::Plains, Card::Swamp, Card::Island])
    .battlefield(P0, &[Card::Island])
    .hand(P0, &[Card::Brainstorm, Card::Mountain])
    .actions(&[
        ME.casts(Card::Brainstorm),
        Resolve,
        ME.picks(Card::Mountain),
        ME.picks(Card::Plains),
        ME.library_top(&[Card::Plains, Card::Mountain]),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.has(Zone::Hand, Card::Plains, 1),
        ME.library_top(&[Card::Mountain]),
    ])];
}
