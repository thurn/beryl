// Generated from the card store by `just card header`: notes go below the end marker.
//! Leonin Scimitar {1}
//! Artifact — Equipment
//!
//! Equipped creature gets +1/+1.
//! Equip {1} ({1}: Attach to target creature you control. Equip only as a sorcery.)
// End of the generated card header.

use crate::prelude::*;

/// "Equipped creature gets +1/+1": a continuous effect on the creature it is attached to
/// ([CR 301.5a]): "The creature an Equipment is attached to is called the 'equipped creature.'"
///
/// Implemented so that scenarios have an Equipment to attach: it is in none of the decks.
///
/// [CR 301.5a]: https://yawgatog.com/resources/magic-rules/#R3015a
pub static CARD: CardDef = Printed::LeoninScimitar
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(1, 1)],
        }),
        Ability::equip(mana("{1}")),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SCIMITAR: &str = Card::LeoninScimitar;

    pub const SCENARIOS: &[Scenario] = &[
        // Equip {1} attaches it to a creature its controller controls, which gets +1/+1. The
        // opponent's creature is not a legal target (CR 702.6a: "target creature you
        // control"), so P0 chooses between its own two. `Prop::Creature` is a 3/1.
        Scenario::new("Leonin Scimitar equips a creature, which gets +1/+1")
            .battlefield(
                P0,
                &[SCIMITAR, Prop::Creature, Prop::SmallCreature, Card::Plains],
            )
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(SCIMITAR, 1), // Equip {1}
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Prop::Creature),
                Resolve,
                ME.pt(Prop::Creature, 4, 2),
            ]),
    ];
}
