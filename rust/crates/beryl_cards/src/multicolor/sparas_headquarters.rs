//! Spara's Headquarters. Land — Forest Plains Island. "({T}: Add {G}, {W}, or {U}.) This land
//! enters tapped. Cycling {3} ({3}, Discard this card: Draw a card.)"

use beryl_engine::card::{
    Ability, CardDef, Face, ManaColor, ManaCost, ManaSet, Replacement, Subtype, Subtypes, Types,
};

use crate::names;

/// Its three basic land types give it their intrinsic mana abilities
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],'".
/// Cycling ([CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)) "functions
/// only while the card with cycling is in a player's hand."
pub static CARD: CardDef = CardDef {
    name: names::SPARAS_HEADQUARTERS,
    faces: &[Face {
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Forest, Subtype::Plains, Subtype::Island]),
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::Green)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
            Ability::replacing(Replacement::EntersTapped),
            Ability::cycling(ManaCost::generic(3)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::Seat;
    use beryl_engine::Zone;

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    const THREE_PLAINS: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    /// The cycling ability is the face's fifth.
    const CYCLING: u8 = 4;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Spara's Headquarters enters tapped")
            .hand(P0, &[names::SPARAS_HEADQUARTERS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::PlayLand {
                    player: P0,
                    card: names::SPARAS_HEADQUARTERS,
                },
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::SPARAS_HEADQUARTERS,
                    count: 1,
                }),
            ]),
        Scenario::new("Cycling Spara's Headquarters")
            .battlefield(P0, THREE_PLAINS)
            .hand(P0, &[names::SPARAS_HEADQUARTERS])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, names::SPARAS_HEADQUARTERS, CYCLING),
                count(P0, Zone::Hand, names::ISLAND, 1),
                count(P0, Zone::Graveyard, names::SPARAS_HEADQUARTERS, 1),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 3,
                }),
            ]),
        // Its {U} is the only blue mana for Timetwister.
        Scenario::new("Spara's Headquarters' blue mana")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::SPARAS_HEADQUARTERS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[names::TIMETWISTER])
            .library(P0, &[names::ISLAND; 7])
            .library(P1, &[names::ISLAND; 8])
            .actions(&[
                Action::cast(P0, names::TIMETWISTER),
                count(P0, Zone::Graveyard, names::TIMETWISTER, 1),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::SPARAS_HEADQUARTERS,
                    count: 1,
                }),
            ]),
        // Its {W} is the only white mana for the artifact. The land still to play keeps the game
        // in P0's main phase.
        Scenario::new("Spara's Headquarters' white mana")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::SPARAS_HEADQUARTERS),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[props::ARTIFACT, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::SPARAS_HEADQUARTERS,
                    count: 1,
                }),
                Action::Resolve,
                count(P0, Zone::Battlefield, props::ARTIFACT, 1),
            ]),
    ];
}
