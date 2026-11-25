// Generated from the card store by `just card header`: notes go below the end marker.
//! Wild Nacatl {G}
//! Creature — Cat Warrior, 1/1
//!
//! This creature gets +1/+1 as long as you control a Mountain.
//! This creature gets +1/+1 as long as you control a Plains.
// End of the generated card header.

use crate::prelude::*;

/// The bonuses are two separate modifications. A land with both types supplies both.
/// A land that Blood Moon makes a Mountain counts because land types are set in layer 4
/// ([CR 613.1d]) before the power and toughness modifications in layer 7c ([CR 613.4c]).
///
/// [CR 613.1d]: https://yawgatog.com/resources/magic-rules/#R6131d
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::WildNacatl
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(exists(&permanents(Filter::All(&[
                Filter::Subtype(Subtype::Mountain),
                yours(),
            ])))),
            modifications: &[Modification::ModifyPt(1, 1)],
        }),
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(exists(&permanents(Filter::All(&[
                Filter::Subtype(Subtype::Plains),
                yours(),
            ])))),
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
