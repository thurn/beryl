// Generated from the card store by `just card header`: notes go below the end marker.
//! Hibernation Sliver {U}{B}
//! Creature — Sliver, 2/2
//!
//! All Slivers have "Pay 2 life: Return this permanent to its owner's hand."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HibernationSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::Subtype(Subtype::Sliver)),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[
        Ability::activated(Cost::of_parts(&[CostPart::Life(2)])).effect(&return_to_hand(this()))
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("non-Sliver permanent does not gain the ability")
                .battlefield(P0, &[Card::HibernationSliver, Prop::Vanilla])
                .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        ];
}
