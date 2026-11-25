// Generated from the card store by `just card header`: notes go below the end marker.
//! Horizon Scholar {5}{U}
//! Creature — Sphinx, 4/4
//!
//! Flying
//! When this creature enters, scry 2. (Look at the top two cards of your library, then put any
//! number of them on the bottom and the rest on top in any order.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HorizonScholar
    .keywords(Keywords::FLYING)
    .abilities(&[when_enters(&scry(you(), 2))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("another permanent entering does not scry")
            .battlefield(P0, &[Card::HorizonScholar])
            .hand(P0, &[Card::Island])
            .actions(&[ME.plays(Card::Island), stack(0)]),
        Scenario::new("entering scries 2: one card to the bottom, the other stays on top")
            .hand(P0, &[Card::HorizonScholar])
            .battlefield(P0, &[Card::Island; 6])
            .library(P0, &[Card::Plains, Prop::Artifact, Card::Island])
            .actions(&[
                ME.casts(Card::HorizonScholar),
                Resolve,
                // The enters trigger is on the stack above nothing else; resolving it scries.
                Resolve,
                ME.picks_up_to(Prop::Artifact),
                ME.done_picking(),
                ME.library_top(&[Card::Plains, Card::Island, Prop::Artifact]),
            ]),
    ];
}
