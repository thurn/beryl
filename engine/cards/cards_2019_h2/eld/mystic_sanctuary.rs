// Generated from the card store by `just card header`: notes go below the end marker.
//! Mystic Sanctuary
//! Land — Island
//!
//! ({T}: Add {U}.)
//! This land enters tapped unless you control three or more other Islands.
//! When this land enters untapped, you may put target instant or sorcery card from your graveyard
//! on top of your library.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MysticSanctuary
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::replacing(&Replacement::enters_tapped_unless_you_control(
            3,
            &Filter::Subtype(Subtype::Island),
        )),
        Ability::triggered(&Trigger::ENTERS_UNTAPPED)
            .target(&TargetSpec::graveyard_card(Filter::All(&[
                instant_or_sorcery(),
                yours(),
            ])))
            .effect(&may(
                you(),
                &put_into_library(target(), LibraryPosition::Top),
            )),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Mystic Sanctuary with three other Islands puts an instant on top")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::MysticSanctuary])
            .graveyard(P0, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::MysticSanctuary),
                ME.tapped(Card::MysticSanctuary, 0),
                ME.yes(),
                ME.library_top(&[Card::Counterspell]),
            ]),
        // Two Islands and a Plains: three other lands, but only two of them Islands. It
        // enters tapped, so its trigger never exists and the stack stays empty.
        Scenario::new("Mystic Sanctuary with two other Islands enters tapped and does nothing")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[Card::MysticSanctuary])
            .graveyard(P0, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::MysticSanctuary),
                ME.tapped(Card::MysticSanctuary, 1),
                stack(0),
                ME.has(Zone::Graveyard, Card::Counterspell, 1),
            ]),
    ];
}
