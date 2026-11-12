//! Tundra. Land — Plains Island. "({T}: Add {W} or {U}.)"

use beryl_engine::card::{Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Types};

use crate::names;

/// Not basic, but it has both basic land types, and with them both intrinsic mana abilities
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' even if
/// the text box doesn't actually contain that text". They are mana abilities
/// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): an activated ability
/// that "doesn't require a target ..., it could add mana to a player's mana pool when it
/// resolves, it's not a loyalty ability ..., and its cost and effect don't move any card to or
/// from a library."
pub static CARD: CardDef = CardDef {
    name: names::TUNDRA,
    faces: &[Face {
        types: Types::LAND,
        subtypes: Subtypes::of(&[Subtype::Plains, Subtype::Island]),
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{DecisionKind, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, Scenario};

    use crate::names;

    /// Tundra is not a Plains or an Island card, so it is a candidate of its own beside them.
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("Tundra played beside a Plains in hand")
        .hand(P0, &[names::TUNDRA, names::PLAINS])
        .actions(&[
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 3,
                },
            },
            Action::PlayLand {
                player: P0,
                card: names::TUNDRA,
            },
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::PostcombatMain,
            },
            Action::Assert {
                assertion: Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(names::TUNDRA),
                    count: 1,
                },
            },
            Action::Assert {
                assertion: Assertion::Tapped {
                    player: P0,
                    card: names::TUNDRA,
                    count: 0,
                },
            },
            Action::Assert {
                assertion: Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(names::PLAINS),
                    count: 1,
                },
            },
        ])];
}
