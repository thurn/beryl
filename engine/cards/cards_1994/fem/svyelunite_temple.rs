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

    pub const SCENARIOS: &[Scenario] =
        &[
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
        ];
}
