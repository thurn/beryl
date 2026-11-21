// Generated from the card store by `just card header`: notes go below the end marker.
//! Life and Limb {3}{G}
//! Enchantment
//!
//! All Forests and all Saprolings are 1/1 green Saproling creatures and Forest lands in addition to
//! their other types. (They're affected by summoning sickness.)
// End of the generated card header.

use crate::prelude::*;

/// Life and Limb depends on Blood Moon in layer 4 ([CR 613.8a]). Apply Blood Moon first
/// regardless of timestamps. A Dryad Arbor it changes to a Mountain no longer qualifies
/// as a Forest.
///
/// [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a
pub static CARD: CardDef = Printed::LifeAndLimb
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::AnyOf(&[
                Filter::Subtype(Subtype::Forest),
                Filter::Subtype(Subtype::Saproling),
            ]),
        },
        condition: None,
        modifications: &[
            Modification::AddTypes(Types::CREATURE.union(Types::LAND)),
            Modification::AddSubtype(Subtype::Saproling),
            Modification::AddSubtype(Subtype::Forest),
            Modification::SetColors(Colors::GREEN),
            Modification::SetBasePt(&Amount::Fixed(1), &Amount::Fixed(1)),
        ],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LIFE_AND_LIMB: &str = Card::LifeAndLimb;

    // Life and Limb costs {3}{G}, which no land in a scenario pays for, so it starts on the
    // battlefield. A Plains in P0's hand keeps the game in P0's main phase where nothing else
    // would give P0 a decision.
    pub const SCENARIOS: &[Scenario] = &[
        // The Forest is basic, so Blood Moon leaves it a Forest, and Life and Limb still
        // makes it a creature.
        Scenario::new("a basic Forest is a creature under Blood Moon")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[LIFE_AND_LIMB, Card::BloodMoon, Card::Forest])
            .actions(&[ME.creature(Card::Forest, true)]),
        // Under Blood Moon Dryad Arbor is a Mountain and no Forest, so what Life and Limb
        // adds to Forests does not reach it and it taps for {R} alone. Whichever came first,
        // this effect is applied after Blood Moon's (CR 613.8a).
        Scenario::new("Dryad Arbor under Blood Moon is no Forest for it")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[LIFE_AND_LIMB, Card::BloodMoon, Card::DryadArbor])
            .actions(&[ME.creature(Card::DryadArbor, true)]),
        Scenario::new("Blood Moon coming after leaves Dryad Arbor no Forest")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[Card::DryadArbor, LIFE_AND_LIMB, Card::BloodMoon])
            .actions(&[ME.creature(Card::DryadArbor, true)]),
    ];
}
