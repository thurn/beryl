// Generated from the card store by `just card header`: notes go below the end marker.
//! Harmonic Sliver {1}{G}{W}
//! Creature — Sliver, 1/1
//!
//! All Slivers have "When this permanent enters, destroy target artifact or enchantment."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HarmonicSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::Subtype(Subtype::Sliver)),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::permanent(Filter::Types(
            Types::ARTIFACT.union(Types::ENCHANTMENT),
        )))
        .effect(&destroy(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // The trigger destroys, and a regeneration shield saves its target. P1 keeps a second
        // shield so that they still have something to do once the first resolves.
        Scenario::new("Harmonic Sliver's trigger allows its target to regenerate")
            .battlefield(P0, &[Card::Forest, Card::Forest, Card::Plains])
            .hand(P0, &[Card::HarmonicSliver])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::Ornithopter])
            .hand(P1, &[Prop::Regeneration, Prop::Regeneration])
            .actions(&[
                ME.casts(Card::HarmonicSliver),
                Resolve,
                ME.passes(),
                THEM.casts(Prop::Regeneration),
                THEM.targets(Card::Ornithopter),
                Resolve,
                Resolve,
                THEM.controls(Card::Ornithopter, 1),
                THEM.tapped(Card::Ornithopter, 1),
            ]),
    ];
}
