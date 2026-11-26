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
        // {1}{U}{U} from the Temple's {U}{U} and one of Mana Crypt's {C}{C}: the
        // sacrificed Temple must spend both its mana, and the Crypt the rest.
        Scenario::new("Svyelunite Temple and Mana Crypt pay for Sink into Stupor")
            .interaction(&[Card::ManaCrypt, Card::SinkIntoStupor])
            .battlefield(P0, &[Card::SvyeluniteTemple, Card::ManaCrypt])
            .battlefield(P1, &[Card::MotherOfRunes])
            .hand(P0, &[Card::SinkIntoStupor])
            .actions(&[
                ME.casts(Card::SinkIntoStupor),
                Action::Resolve,
                THEM.has(Zone::Hand, Card::MotherOfRunes, 1),
                ME.has(Zone::Graveyard, Card::SvyeluniteTemple, 1),
            ]),
    ];
}
