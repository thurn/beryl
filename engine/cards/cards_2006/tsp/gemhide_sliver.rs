// Generated from the card store by `just card header`: notes go below the end marker.
//! Gemhide Sliver {1}{G}
//! Creature — Sliver, 1/1
//!
//! All Slivers have "{T}: Add one mana of any color."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::GemhideSliver
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(Filter::Subtype(Subtype::Sliver)),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[Ability::tap_for_mana(ManaSet::any_color(1))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GEMHIDE: &str = Card::GemhideSliver;

    // The rules library's mana scenarios show the rest: a summoning-sick Sliver, an opponent's
    // Sliver, two Gemhide Slivers and a {U} paid.
    pub const SCENARIOS: &[Scenario] = &[
        // Portable Hole costs {W}, which the Island can't pay.
        Scenario::new("The granted mana pays {W}")
            .battlefield(P0, &[GEMHIDE, Card::Island])
            .hand(P0, &[Prop::Artifact])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                ME.controls(Prop::Artifact, 1),
                ME.tapped(GEMHIDE, 1),
                ME.tapped(Card::Island, 0),
            ]),
    ];
}
