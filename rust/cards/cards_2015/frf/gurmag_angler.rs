// Generated from the card store by `just card header`: notes go below the end marker.
//! Gurmag Angler {6}{B}
//! Creature — Zombie Fish, 5/5
//!
//! Delve (Each card you exile from your graveyard while casting this spell pays for {1}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::GurmagAngler.keywords(Keywords::DELVE).card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ANGLER: &str = Card::GurmagAngler;
    const TRINISPHERE: &str = Card::Trinisphere;

    pub const SCENARIOS: &[Scenario] = &[
        // Six of the seven cards pay {6}, and the Swamp pays {B}. Delve pays generic mana
        // only, so the seventh card stays.
        Scenario::new("Gurmag Angler with seven cards in the graveyard costs {B}")
            .battlefield(P0, &[Card::Swamp])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island; 7])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Its total cost, {6}{B}, is three or more, so Trinisphere leaves it alone, and delve
        // pays the generic mana of the total after it is locked in.
        Scenario::new("Gurmag Angler under Trinisphere with a full graveyard")
            .interaction(&[TRINISPHERE])
            .battlefield(P0, &[TRINISPHERE, Card::Swamp])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island; 7])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Gurmag Angler is a Zombie Fish, so the colored mana of a Cavern of Souls that chose
        // Zombie may pay its {B}, and six cards pay {6}: the Cavern is the only land.
        Scenario::new("Gurmag Angler paid with delve and Cavern of Souls' mana")
            .interaction(&[Card::CavernOfSouls])
            .hand(P0, &[Card::CavernOfSouls, ANGLER])
            .graveyard(P0, &[Card::Island; 6])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Zombie"),
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.tapped(Card::CavernOfSouls, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Exiling six cards pays the generic cost and leaves the printed mana
        // value at seven for Mana Drain's delayed trigger.
        Scenario::new("Gurmag Angler cast for one mana still drains for seven")
            .interaction(&[Card::ManaDrain])
            .battlefield(P0, &[Card::Swamp])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island; 6])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::ManaDrain, Card::Plains])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                THEM.casts(Card::ManaDrain),
                ME.has(Zone::Graveyard, ANGLER, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                THEM.mana(ManaColor::Colorless, 7),
            ]),
        // Escape supplies the alternative cost. Delve pays its generic mana with
        // six cards, and escape's separate cost takes three more.
        Scenario::new("Gurmag Angler can delve while escaping")
            .interaction(&[Card::UnderworldBreach])
            .battlefield(P0, &[Card::UnderworldBreach, Card::Swamp])
            .graveyard(
                P0,
                &[
                    ANGLER,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.escapes(ANGLER),
                ME.tapped(Card::Swamp, 1),
                ME.has(Zone::Exile, Card::Island, 9),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
    ];
}
