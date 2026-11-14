// Generated from the card store by `just card header`: notes go below the end marker.
//! Wild Nacatl {G}
//! Creature — Cat Warrior, 1/1
//!
//! This creature gets +1/+1 as long as you control a Mountain.
//! This creature gets +1/+1 as long as you control a Plains.
// End of the generated card header.

use crate::prelude::*;

/// Each "as long as you control a [land type]" is its own static ability, a layer 7c
/// modification of this creature ([CR 613.4c]): "Effects and counters that modify power
/// and/or toughness (but don't set power and/or toughness to a specific number or value) are
/// applied."
/// The condition is read from the permanents as the layers before 7c left them, so a land
/// that a layer 4 effect makes a Mountain counts ([CR 613.1d]): "Layer 4: Type-changing
/// effects are applied." The two abilities are separate, so a land that is both a Mountain
/// and a Plains gives both bonuses.
///
/// [CR 613.1d]: https://yawgatog.com/resources/magic-rules/#R6131d
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::WildNacatl
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(exists(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Subtype(Subtype::Mountain),
                    Filter::Whose(&Players::You),
                ]),
            })),
            modifications: &[Modification::ModifyPt(1, 1)],
        }),
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(exists(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Subtype(Subtype::Plains),
                    Filter::Whose(&Players::You),
                ]),
            })),
            modifications: &[Modification::ModifyPt(1, 1)],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const NACATL: &str = Card::WildNacatl;

    // Wild Nacatl costs {G}, which no land in a scenario makes, so it starts on the
    // battlefield.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("no Mountain and no Plains leaves it a 1/1")
            .battlefield(P0, &[NACATL, Card::Island])
            .actions(&[ME.pt(NACATL, 1, 1)]),
        Scenario::new("a Mountain gives +1/+1")
            .battlefield(P0, &[NACATL, Card::Mountain])
            .actions(&[ME.pt(NACATL, 2, 2)]),
        Scenario::new("a Plains gives +1/+1")
            .battlefield(P0, &[NACATL, Card::Plains])
            .actions(&[ME.pt(NACATL, 2, 2)]),
        Scenario::new("a Mountain and a Plains give +2/+2")
            .battlefield(P0, &[NACATL, Card::Mountain, Card::Plains])
            .actions(&[ME.pt(NACATL, 3, 3)]),
        // The opponent's Mountain is not one "you control".
        Scenario::new("an opponent's Mountain gives nothing")
            .battlefield(P0, &[NACATL])
            .battlefield(P1, &[Card::Mountain])
            .actions(&[ME.pt(NACATL, 1, 1)]),
        // Tundra is a Plains Island, so it gives only one bonus.
        Scenario::new("a nonbasic land of two types gives each bonus it has")
            .battlefield(P0, &[NACATL, Card::Tundra])
            .actions(&[ME.pt(NACATL, 2, 2)]),
        // Blood Moon makes Tundra a Mountain, so Tundra gives the Mountain bonus, and the
        // basic Plains still gives the Plains bonus.
        Scenario::new("a land Blood Moon makes a Mountain counts as one")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[NACATL, Card::Plains, Card::Tundra, Card::BloodMoon])
            .actions(&[ME.pt(NACATL, 3, 3)]),
        // Under Blood Moon Tundra is no longer a Plains, so with a Mountain alone there is
        // no Plains: only the Mountain bonus remains.
        Scenario::new("a land Blood Moon makes a Mountain is no longer a Plains")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[NACATL, Card::Tundra, Card::BloodMoon])
            .actions(&[ME.pt(NACATL, 2, 2)]),
    ];
}
