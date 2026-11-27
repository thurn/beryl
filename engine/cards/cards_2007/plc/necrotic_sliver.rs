// Generated from the card store by `just card header`: notes go below the end marker.
//! Necrotic Sliver {1}{W}{B}
//! Creature — Sliver, 2/2
//!
//! All Slivers have "{3}, Sacrifice this permanent: Destroy target permanent."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::NecroticSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::Subtype(Subtype::Sliver)),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[
        Ability::activated(Cost::of(ManaCost::generic(3), &[CostPart::SACRIFICE]))
            .target(&TargetSpec::permanent(Filter::Any))
            .effect(&destroy(target())),
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
                        Card::NecroticSliver,
                        Prop::Vanilla,
                        Card::Plains,
                        Card::Island,
                        Card::Swamp,
                    ],
                )
                .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        ];
}
