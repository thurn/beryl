// Generated from the card store by `just card header`: notes go below the end marker.
//! Seachrome Coast
//! Land
//!
//! This land enters tapped unless you control two or fewer other lands.
//! {T}: Add {W} or {U}.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::{Ability, CardDef, ManaColor, ManaSet, Replacement, card};

/// The entering replacement ([CR 614.1d](https://yawgatog.com/resources/magic-rules/#R6141d)):
/// "Continuous effects that read "[This permanent] enters . . ." ... are replacement
/// effects." It counts the lands its controller controls as it would enter, which are all
/// other lands.
pub static CARD: CardDef = card(
    names::SEACHROME_COAST,
    &[printed::SEACHROME_COAST.abilities(&[
        Ability::replacing(Replacement::EntersTappedUnlessFewLands(2)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::Zone;
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

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
            .permanents(
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
            .permanents(
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
        // Its {W} is P0's only white mana for the artifact. The land still to play keeps the
        // game in P0's main phase.
        Scenario::new("Seachrome Coast's white mana")
            .permanents(
                P0,
                &[
                    Permanent::new(names::SEACHROME_COAST),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[props::ARTIFACT, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                coast_tapped(1),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::ARTIFACT,
                    count: 1,
                }),
            ]),
    ];
}
