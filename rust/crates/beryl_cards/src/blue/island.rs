//! Island. Basic Land — Island.

use crate::names;
use beryl_card::{
    Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Subtypes, Supertypes, Types, card,
};

/// [CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056): "An object with the land
/// card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],' ... for
/// Islands, {U}".
pub static CARD: CardDef = card(
    names::ISLAND,
    &[Face::EMPTY
        .supertypes(Supertypes::BASIC)
        .types(Types::LAND)
        .subtypes(Subtypes::of(&[Subtype::Island]))
        .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, TurnStep, Zone};
    use beryl_scenario_types::{Action, Assertion, P0, P1, Pick, Scenario};

    use crate::names;

    /// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
    /// may play a land card from their hand during a main phase of their turn when the stack is
    /// empty." Not in another player's turn: when the active player passes, the other player
    /// with an Island in hand has nothing to decide.
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("an Island only in its owner's main phase")
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[names::ISLAND])
        .hand(P1, &[names::ISLAND])
        .actions(&[
            Action::Answer {
                player: P1,
                kind: DecisionKind::Priority,
                pick: Pick::Pass,
            },
            // Then straight to the active player's next main phase.
            Action::Assert {
                assertion: Assertion::Decision {
                    player: P1,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                },
            },
            Action::PlayLand {
                player: P1,
                card: names::ISLAND,
            },
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::End,
            },
            Action::Assert {
                assertion: Assertion::Count {
                    player: P1,
                    zone: Zone::Battlefield,
                    card: Some(names::ISLAND),
                    count: 1,
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
