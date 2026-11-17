// Generated from the card store by `just card header`: notes go below the end marker.
//! Liliana's Shade {2}{B}{B}
//! Creature — Shade, 1/1
//!
//! When this creature enters, you may search your library for a Swamp card, reveal it, put it into
//! your hand, then shuffle.
//! {B}: This creature gets +1/+1 until end of turn.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that check whether Urborg affects lands in a library.
pub static CARD: CardDef = Printed::LilianasShade
    .abilities(&[
        when_enters(&may(
            you(),
            &search(
                Players::You,
                Filter::Subtype(Subtype::Swamp),
                Found::RevealToHand,
            ),
        )),
        Ability::activated(Cost::mana(mana("{B}"))).effect(&modify(
            this(),
            &[Modification::ModifyPt(1, 1)],
            Duration::EndOfTurn,
        )),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SHADE: &str = Card::LilianasShade;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the Shade finds a Swamp and spends black mana to grow")
            .hand(P0, &[SHADE, Card::Swamp])
            .battlefield(P0, &[Card::Swamp; 5])
            .library(P0, &[Card::Swamp, Card::Island])
            .actions(&[
                ME.casts(SHADE),
                Resolve,
                Resolve,
                ME.yes(),
                ME.picks_up_to(Card::Swamp),
                ME.has(Zone::Hand, Card::Swamp, 2),
                ME.activates(SHADE, 1),
                Resolve,
                ME.pt(SHADE, 2, 2),
                advance_to(2, TurnStep::Upkeep),
                ME.pt(SHADE, 1, 1),
            ]),
        // The ability listens for the Shade itself, so the land entering starts no search.
        Scenario::new("playing a land with the Shade in play starts no search")
            .battlefield(P0, &[SHADE])
            .hand(P0, &[Card::Swamp])
            .library(P0, &[Card::Swamp])
            .actions(&[ME.plays(Card::Swamp), stack(0)]),
    ];
}
