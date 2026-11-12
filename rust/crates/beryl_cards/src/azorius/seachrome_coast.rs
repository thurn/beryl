//! Seachrome Coast. Land. "This land enters tapped unless you control two or fewer other
//! lands. {T}: Add {W} or {U}."

use beryl_engine::card::{Ability, CardDef, Face, ManaColor, ManaSet, Replacement, Types};

use crate::names;

/// The entering replacement ([CR 614.1d](https://yawgatog.com/resources/magic-rules/#R6141d)):
/// "Continuous effects that read "[This permanent] enters . . ." ... are replacement
/// effects." It counts the lands its controller controls as it would enter, which are all
/// other lands.
pub static CARD: CardDef = CardDef {
    name: names::SEACHROME_COAST,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::replacing(Replacement::EntersTappedUnlessFewLands(2)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::names;

    const fn coast_tapped(count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player: P0,
            card: names::SEACHROME_COAST,
            count,
        })
    }

    const PLAY: Action = Action::PlayLand {
        player: P0,
        card: names::SEACHROME_COAST,
    };

    pub const SCENARIOS: &[Scenario] = &[
        // Then its {U} is the only blue mana for Timetwister.
        Scenario::new("Seachrome Coast with two other lands enters untapped")
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[names::SEACHROME_COAST, names::TIMETWISTER])
            .library(P0, &[names::ISLAND; 7])
            .library(P1, &[names::ISLAND; 8])
            .actions(&[
                PLAY,
                coast_tapped(0),
                Action::cast(P0, names::TIMETWISTER),
                coast_tapped(1),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(names::TIMETWISTER),
                    count: 1,
                }),
            ]),
        Scenario::new("Seachrome Coast with three other lands enters tapped")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::TUNDRA),
                ],
            )
            .hand(P0, &[names::SEACHROME_COAST])
            .library(P1, &[names::ISLAND])
            .actions(&[PLAY, coast_tapped(1)]),
        // Its {W} is P0's only white mana for Mana Tithe, against P1's Mana Crypt. P1's land
        // still to play keeps the game in their main phase.
        Scenario::new("Seachrome Coast's white mana")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(names::SEACHROME_COAST),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[names::MANA_TITHE])
            .library(P0, &[names::ISLAND])
            .hand(P1, &[names::MANA_CRYPT, names::PLAINS])
            .actions(&[
                Action::cast(P1, names::MANA_CRYPT),
                Action::cast(P0, names::MANA_TITHE),
                coast_tapped(1),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_CRYPT),
                    count: 1,
                }),
            ]),
    ];
}
