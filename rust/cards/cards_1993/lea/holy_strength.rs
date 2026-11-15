// Generated from the card store by `just card header`: notes go below the end marker.
//! Holy Strength {W}
//! Enchantment — Aura
//!
//! Enchant creature
//! Enchanted creature gets +1/+2.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios: a white Aura whose enchanted creature stays a creature, which
/// Mother of Runes's protection from white makes fall off.
///
/// "Enchant creature": an Aura spell targets what it will enchant ([CR 303.4a]): "An Aura
/// spell requires a target, which is defined by its enchant ability."
///
/// "Enchanted creature gets +1/+2." A static ability of the Aura, applied in layer 7c
/// ([CR 613.4c]): "Effects and counters that modify power and/or toughness (but don't set
/// power and/or toughness to a specific number or value) are applied."
///
/// [CR 303.4a]: https://yawgatog.com/resources/magic-rules/#R3034a
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::HolyStrength
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(1, 2)],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HOLY: &str = Card::HolyStrength;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Holy Strength makes its creature 3/3")
            .battlefield(P0, &[Card::Plains, Prop::Vanilla])
            .hand(P0, &[HOLY, Card::Island])
            .actions(&[
                ME.casts(HOLY),
                ME.controls(HOLY, 1),
                ME.pt(Prop::Vanilla, 3, 3),
            ]),
        Scenario::new("Holy Strength enchants an opponent's creature")
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[HOLY, Card::Island])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(HOLY),
                ME.controls(HOLY, 1),
                THEM.pt(Prop::Vanilla, 3, 3),
            ]),
    ];
}
