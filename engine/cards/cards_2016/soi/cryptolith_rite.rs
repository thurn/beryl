// Generated from the card store by `just card header`: notes go below the end marker.
//! Cryptolith Rite {1}{G}
//! Enchantment
//!
//! Creatures you control have "{T}: Add one mana of any color."
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CryptolithRite
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(creature_you_control()),
        condition: None,
        modifications: &[Modification::grant(0)],
    })])
    .grants(&[Ability::tap_for_mana(ManaSet::any_color(1))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("a creature taps for white mana to cast a multicolored spell")
                .battlefield(P0, &[Card::CryptolithRite, Prop::Vanilla, Card::Forest])
                .hand(P0, &[Prop::GreenWhiteCreature])
                .actions(&[
                    ME.casts(Prop::GreenWhiteCreature),
                    Resolve,
                    ME.tapped(Prop::Vanilla, 1),
                    ME.tapped(Card::Forest, 1),
                    ME.controls(Prop::GreenWhiteCreature, 1),
                ]),
        ];
}
