// Generated from the card store by `just card header`: notes go below the end marker.
//! Unmake {W/B}{W/B}{W/B}
//! Instant
//!
//! Exile target creature.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Unmake
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&exile(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Three lands of `lands` beside an Island that pays none of the cost, and a creature of
    /// the opponent's to exile; the lands each color pays with are tapped.
    const fn paid_with(title: &'static str, lands: &'static [&'static str]) -> Scenario {
        Scenario::new(title)
            .hand(P0, &[Card::Unmake])
            .battlefield(P0, lands)
            .battlefield(P1, &[Prop::Vanilla])
    }

    pub const SCENARIOS: &[Scenario] = &[
        paid_with(
            "Unmake's hybrid symbols are paid with white mana",
            &[Card::Plains, Card::Plains, Card::Plains, Card::Island],
        )
        .actions(&[
            ME.casts(Card::Unmake),
            Resolve,
            ME.tapped(Card::Plains, 3),
            ME.tapped(Card::Island, 0),
            THEM.has(Zone::Exile, Prop::Vanilla, 1),
        ]),
        paid_with(
            "Unmake's hybrid symbols are paid with black mana",
            &[Card::Swamp, Card::Swamp, Card::Swamp, Card::Island],
        )
        .actions(&[
            ME.casts(Card::Unmake),
            Resolve,
            ME.tapped(Card::Swamp, 3),
            ME.tapped(Card::Island, 0),
            THEM.has(Zone::Exile, Prop::Vanilla, 1),
        ]),
        paid_with(
            "Unmake's hybrid symbols are paid with white and black mana together",
            &[Card::Plains, Card::Swamp, Card::Swamp, Card::Island],
        )
        .actions(&[
            ME.casts(Card::Unmake),
            Resolve,
            ME.tapped(Card::Plains, 1),
            ME.tapped(Card::Swamp, 2),
            ME.tapped(Card::Island, 0),
            THEM.has(Zone::Exile, Prop::Vanilla, 1),
        ]),
    ];
}
