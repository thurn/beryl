// Generated from the card store by `just card header`: notes go below the end marker.
//! Svyelunite Temple
//! Land
//!
//! This land enters tapped.
//! {T}: Add {U}.
//! {T}, Sacrifice this land: Add {U}{U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SvyeluniteTemple
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::mana(tap_and_sacrifice(), ManaSet::of(ManaColor::Blue, 2)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Svyelunite Temple alone pays for Lord of Atlantis")
            .interaction(&[Card::LordOfAtlantis])
            .battlefield(P0, &[Card::SvyeluniteTemple])
            .hand(P0, &[Card::LordOfAtlantis])
            .actions(&[
                ME.casts(Card::LordOfAtlantis),
                Action::Resolve,
                ME.controls(Card::LordOfAtlantis, 1),
                ME.has(Zone::Graveyard, Card::SvyeluniteTemple, 1),
            ]),
        // Dissolve's {1}{U}{U} from the Temple's {U}{U} and one of Mana Crypt's {C}{C}: the
        // sacrificed Temple must spend both its mana, and the Crypt the rest.
        Scenario::new("Svyelunite Temple and Mana Crypt pay for Dissolve")
            .interaction(&[Card::ManaCrypt, Card::Dissolve, Card::MotherOfRunes])
            .battlefield(P0, &[Card::SvyeluniteTemple, Card::ManaCrypt])
            .hand(P0, &[Card::Dissolve])
            .library(P0, &[Card::Island])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::MotherOfRunes])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Card::MotherOfRunes),
                ME.casts(Card::Dissolve),
                Action::Resolve,
                // Scry 1: the Island stays on top.
                ME.done_picking(),
                THEM.has(Zone::Graveyard, Card::MotherOfRunes, 1),
                ME.has(Zone::Graveyard, Card::SvyeluniteTemple, 1),
            ]),
    ];
}
