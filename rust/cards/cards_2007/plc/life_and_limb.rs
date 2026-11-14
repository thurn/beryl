// Generated from the card store by `just card header`: notes go below the end marker.
//! Life and Limb {3}{G}
//! Enchantment
//!
//! All Forests and all Saprolings are 1/1 green Saproling creatures and Forest lands in addition to
//! their other types. (They're affected by summoning sickness.)
// End of the generated card header.

use crate::prelude::*;

/// "All Forests and all Saprolings are 1/1 green Saproling creatures and Forest lands in
/// addition to their other types" is one static ability with parts in layers 4, 5 and 7b
/// ([CR 613.6]): "If an effect should be applied in different layers and/or sublayers, the
/// parts of the effect each apply in their appropriate ones." In layer 4 it adds the creature
/// and land card types and the Saproling and Forest subtypes ([CR 613.1d]); a land that gains
/// the Forest subtype gains the mana ability of a Forest ([CR 305.6]). In layer 5 the color
/// becomes green ([CR 613.1e]), and in layer 7b the base power and toughness become 1/1
/// ([CR 613.4b]).
///
/// Which permanents it affects is found as the layers before it left them. A Forest that
/// another layer 4 effect makes a Mountain (Blood Moon on Dryad Arbor) is no Forest any
/// more, so this effect depends on that one ([CR 613.8a]): "An effect is said to “depend on”
/// another if (a) it's applied in the same layer (and, if applicable, sublayer) as the
/// other effect; (b) applying the other would change the text or the existence of the first
/// effect, the set of objects it affects, or what it does to any of those objects". It is
/// applied after Blood Moon whichever has the earlier timestamp, and finds no Forest there.
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
/// [CR 613.1d]: https://yawgatog.com/resources/magic-rules/#R6131d
/// [CR 613.1e]: https://yawgatog.com/resources/magic-rules/#R6131e
/// [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
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
        Scenario::new("a Forest is a 1/1 creature land")
            .battlefield(P0, &[LIFE_AND_LIMB, Card::Forest])
            .actions(&[ME.creature(Card::Forest, true), ME.pt(Card::Forest, 1, 1)]),
        // "All Forests": the opponent's too.
        Scenario::new("an opponent's Forest is a creature too")
            .battlefield(P0, &[LIFE_AND_LIMB])
            .hand(P0, &[Card::Plains])
            .battlefield(P1, &[Card::Forest])
            .actions(&[
                THEM.creature(Card::Forest, true),
                THEM.pt(Card::Forest, 1, 1),
            ]),
        // A land that is no Forest is untouched.
        Scenario::new("a land that is no Forest stays a land")
            .battlefield(P0, &[LIFE_AND_LIMB, Card::Island])
            .hand(P0, &[Card::Plains])
            .actions(&[ME.creature(Card::Island, false)]),
        // Dryad Arbor is a Forest and a creature already: it stays a 1/1.
        Scenario::new("Dryad Arbor is still a 1/1 creature")
            .battlefield(P0, &[LIFE_AND_LIMB, Card::DryadArbor])
            .actions(&[
                ME.creature(Card::DryadArbor, true),
                ME.pt(Card::DryadArbor, 1, 1),
            ]),
        // The Forest is basic, so Blood Moon leaves it a Forest, and Life and Limb still
        // makes it a creature.
        Scenario::new("a basic Forest is a creature under Blood Moon")
            .battlefield(P0, &[LIFE_AND_LIMB, Card::BloodMoon, Card::Forest])
            .actions(&[ME.creature(Card::Forest, true)]),
        // Under Blood Moon Dryad Arbor is a Mountain and no Forest, so what Life and Limb
        // adds to Forests does not reach it and it taps for {R} alone. Whichever came first,
        // this effect is applied after Blood Moon's (CR 613.8a).
        Scenario::new("Dryad Arbor under Blood Moon is no Forest for it")
            .battlefield(P0, &[LIFE_AND_LIMB, Card::BloodMoon, Card::DryadArbor])
            .actions(&[ME.creature(Card::DryadArbor, true)]),
        Scenario::new("Blood Moon coming after leaves Dryad Arbor no Forest")
            .battlefield(P0, &[Card::DryadArbor, LIFE_AND_LIMB, Card::BloodMoon])
            .actions(&[ME.creature(Card::DryadArbor, true)]),
    ];
}
