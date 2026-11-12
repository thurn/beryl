//! Floodfarm Verge. Land. "{T}: Add {W}. {T}: Add {U}. Activate only if you control a Plains
//! or an Island."

use crate::names;
use beryl_card::{Ability, CardDef, Face, Filter, ManaColor, ManaSet, Subtype, Types, card};

/// The {U} ability's restriction reads the land's subtypes, so a Tundra, a Plains Island,
/// counts, as a basic does
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "The basic land types are
/// Plains, Island, Swamp, Mountain, and Forest."
pub static CARD: CardDef = card(
    names::FLOODFARM_VERGE,
    &[Face::EMPTY.types(Types::LAND).abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)).if_you_control(&Filter::AnyOf(&[
            Filter::Subtype(Subtype::Plains),
            Filter::Subtype(Subtype::Island),
        ])),
    ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{TurnStep, Zone};
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const VERGE: &str = names::FLOODFARM_VERGE;

    /// P1 casts the artifact in their main phase, with a land still to play.
    const fn artifact_cast() -> Action {
        Action::cast(P1, props::ARTIFACT)
    }

    /// P1's Plains for the artifact.
    const P1_PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS)];

    const fn verge_tapped(count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player: P0,
            card: VERGE,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Tundra is a Plains and an Island: its {U} and the Verge's pay for Counterspell.
        Scenario::new("Floodfarm Verge's {U} with a Tundra")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Permanent::new(VERGE), Permanent::new(names::TUNDRA)])
            .hand(P0, &[names::COUNTERSPELL])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, P1_PLAINS)
            .hand(P1, &[props::ARTIFACT, names::PLAINS])
            .actions(&[
                artifact_cast(),
                Action::cast(P0, names::COUNTERSPELL),
                verge_tapped(1),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
            ]),
        // A second Verge is neither a Plains nor an Island, so each makes only {W}: P0 can't
        // cast Counterspell and has nothing to do, and the artifact resolves.
        Scenario::new("Floodfarm Verge's {U} without a Plains or an Island")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Permanent::new(VERGE), Permanent::new(VERGE)])
            .hand(P0, &[names::COUNTERSPELL])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, P1_PLAINS)
            .hand(P1, &[props::ARTIFACT, names::PLAINS])
            .actions(&[
                artifact_cast(),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: props::ARTIFACT,
                    count: 1,
                }),
                verge_tapped(0),
            ]),
        // A basic Plains enables the {U}: with the Plains's {W} for the generic mana, the
        // two-drop creature.
        Scenario::new("Floodfarm Verge's {U} with a Plains")
            .battlefield(P0, &[Permanent::new(VERGE), Permanent::new(names::PLAINS)])
            .hand(P0, &[props::SMALL_CREATURE])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::SMALL_CREATURE),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::SMALL_CREATURE,
                    count: 1,
                }),
            ]),
        // Alone, its {W} pays for the artifact. The land still to play keeps the game in P0's
        // main phase.
        Scenario::new("Floodfarm Verge's {W}")
            .battlefield(P0, &[Permanent::new(VERGE)])
            .hand(P0, &[props::ARTIFACT, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                verge_tapped(1),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::ARTIFACT,
                    count: 1,
                }),
            ]),
    ];
}
