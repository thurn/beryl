// Generated from the card store by `just card header`: notes go below the end marker.
//! Acidic Sliver {B}{R}
//! Creature — Sliver, 2/2
//!
//! All Slivers have "{2}, Sacrifice this permanent: This permanent deals 2 damage to any target."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AcidicSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::Subtype(Subtype::Sliver)),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[
        Ability::activated(Cost::of(ManaCost::generic(2), &[CostPart::SACRIFICE]))
            .target(&TargetSpec::any_target())
            .effect(&deal_damage_to_any_target(2)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("non-Sliver permanent does not gain the ability")
                .battlefield(
                    P0,
                    &[
                        Card::AcidicSliver,
                        Prop::Vanilla,
                        Card::Plains,
                        Card::Island,
                    ],
                )
                .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        ];
}
