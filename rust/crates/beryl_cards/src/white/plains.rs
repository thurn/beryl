//! Plains. Basic Land — Plains.

use beryl_engine::card::{
    Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Supertypes, Types,
};

use crate::names;

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... For
/// Plains, [mana symbol] is {W}".
pub static CARD: CardDef = CardDef {
    name: names::PLAINS,
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains]),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::White))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Scenario};

    use crate::names;

    /// [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can normally play
    /// one land during their turn". Two Plains in hand are one candidate (§7.2).
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("one Plains a turn, untapped")
        .hand(P0, &[names::PLAINS, names::PLAINS])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                },
            },
            Action::PlayLand {
                player: P0,
                card: names::PLAINS,
            },
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::PostcombatMain,
            },
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                },
            },
            Action::Assert {
                assertion: Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 0,
                },
            },
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            // The other Plains and the Island drawn this turn.
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 3,
                },
            },
            Action::PlayLand {
                player: P0,
                card: names::PLAINS,
            },
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PostcombatMain,
            },
            Action::Assert {
                assertion: Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(names::PLAINS),
                    count: 2,
                },
            },
            Action::Assert {
                assertion: Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(names::ISLAND),
                    count: 1,
                },
            },
        ])];
}
