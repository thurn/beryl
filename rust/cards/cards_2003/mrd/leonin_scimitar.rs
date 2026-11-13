// Generated from the card store by `just card header`: notes go below the end marker.
//! Leonin Scimitar {1}
//! Artifact — Equipment
//!
//! Equipped creature gets +1/+1.
//! Equip {1} ({1}: Attach to target creature you control. Equip only as a sorcery.)
// End of the generated card header.

use crate::{names, printed};
use beryl_card::{Ability, CardDef, LayerSpec, Modification, Objects, card, mana};

/// "Equipped creature gets +1/+1": a continuous effect on the creature it is attached to
/// ([CR 301.5a](https://yawgatog.com/resources/magic-rules/#R3015a)): "Some Equipment have
/// abilities that affect the equipped creature or the equipped creature's controller."
static EQUIPPED_GETS_ONE_ONE: LayerSpec = LayerSpec {
    affected: Objects::AttachedTo,
    condition: None,
    modification: Modification::ModifyPt(1, 1),
};

/// Implemented so that scenarios have an Equipment to attach: it is in none of the decks.
pub static CARD: CardDef = card(
    names::LEONIN_SCIMITAR,
    &[printed::LEONIN_SCIMITAR.abilities(&[
        Ability::static_ability(&EQUIPPED_GETS_ONE_ONE),
        Ability::equip(mana("{1}")),
    ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::DecisionKind;
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const SCIMITAR: &str = names::LEONIN_SCIMITAR;

    /// A 3/1 to equip.
    const CREATURE: &str = props::CREATURE;

    pub const SCENARIOS: &[Scenario] = &[
        // Equip {1} attaches it to a creature its controller controls, which gets +1/+1. The
        // opponent's creature is not a legal target (CR 702.6a: "target creature you
        // control"), so P0 chooses between its own two.
        Scenario::new("Leonin Scimitar equips a creature, which gets +1/+1")
            .battlefield(
                P0,
                &[
                    Permanent::new(SCIMITAR),
                    Permanent::new(CREATURE),
                    Permanent::new(props::SMALL_CREATURE),
                    Permanent::new(names::PLAINS),
                ],
            )
            .battlefield(P1, &[Permanent::new(props::SMALL_CREATURE)])
            .library(P0, &[names::PLAINS])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::activate(P0, SCIMITAR, 1),
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P0,
                        kind: DecisionKind::Target,
                        candidates: 2,
                    },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Target,
                    pick: Pick::Card { card: CREATURE },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: CREATURE,
                        token: false,
                        power: 4,
                        toughness: 2,
                    },
                },
            ]),
    ];
}
