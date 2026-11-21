// Generated from the card store by `just card header`: notes go below the end marker.
//! Vigilance {W}
//! Enchantment — Aura
//!
//! Enchant creature
//! Enchanted creature has vigilance. (Attacking doesn't cause it to tap.)
// End of the generated card header.

use crate::prelude::*;

/// The Aura gets a new timestamp each time it attaches ([CR 613.7e]). A later Dress Down
/// removes the keyword, and attaching the Aura after Dress Down grants it again.
///
/// [CR 613.7e]: https://yawgatog.com/resources/magic-rules/#R6137e
pub static CARD: CardDef = Printed::Vigilance
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::AddKeywords(Keywords::VIGILANCE)],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const VIGILANCE: &str = Card::Vigilance;
    const DRESS_DOWN: &str = Card::DressDown;

    pub const SCENARIOS: &[Scenario] = &[
        // Dress Down, cast with flash after the Aura was attached, has the later timestamp:
        // the creature loses vigilance, though the Aura stays attached.
        Scenario::new("Dress Down after the Aura takes vigilance away")
            .interaction(&[DRESS_DOWN])
            .battlefield(P0, &[Card::Plains, Card::Island, Card::Island, Prop::Flier])
            .hand(P0, &[VIGILANCE, DRESS_DOWN])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(VIGILANCE),
                Resolve,
                ME.casts(DRESS_DOWN),
                Resolve,
                stack(1),
                Resolve,
                ME.controls(VIGILANCE, 1),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Flier),
                ME.tapped(Prop::Flier, 1),
            ]),
    ];
}
