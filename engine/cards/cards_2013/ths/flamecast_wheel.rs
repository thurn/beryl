// Generated from the card store by `just card header`: notes go below the end marker.
//! Flamecast Wheel {1}
//! Artifact
//!
//! {5}, {T}, Sacrifice this artifact: It deals 3 damage to target creature.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::FlamecastWheel
    .abilities(&[
        Ability::activated(Cost::of(mana("{5}"), &[CostPart::TAP, CostPart::SACRIFICE]))
            .target(&TargetSpec::creature())
            .effect(&Effect::DealDamage {
                to: Recipients::Permanents(target()),
                amount: Amount::Fixed(3),
            }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WHEEL: &str = Card::FlamecastWheel;
    const GIANT: &str = Card::HillGiant;
    const ELEMENTAL: &str = Card::AirElemental;

    pub const SCENARIOS: &[Scenario] = &[
        // 3 damage is exactly lethal to a 3/3, and the Wheel is sacrificed as a cost.
        Scenario::new("Flamecast Wheel deals 3 damage and is sacrificed")
            .battlefield(
                P0,
                &[
                    WHEEL,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[GIANT])
            .actions(&[
                ME.activates(WHEEL, 0),
                Resolve,
                THEM.has(Zone::Graveyard, GIANT, 1),
                ME.has(Zone::Graveyard, WHEEL, 1),
            ]),
        // 3 damage is not enough for a 4/4.
        Scenario::new("Flamecast Wheel's 3 damage does not kill a 4/4")
            .battlefield(
                P0,
                &[
                    WHEEL,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[ELEMENTAL])
            .actions(&[
                ME.activates(WHEEL, 0),
                Resolve,
                THEM.has(Zone::Battlefield, ELEMENTAL, 1),
                ME.has(Zone::Graveyard, WHEEL, 1),
            ]),
        // {T} is a cost, so a tapped Wheel cannot be activated even with {5} and a target
        // available. P0's only actions are then mana abilities, so the engine gives P0 no
        // priority and the first decision is P1's. A Wheel that could be activated would
        // stop at P0's priority instead.
        Scenario::new("Flamecast Wheel cannot be activated while tapped")
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .permanents(P0, &[Permanent::new(WHEEL).tapped()])
            .battlefield(P1, &[GIANT])
            .actions(&[THEM.deciding(DecisionKind::Priority, 2)]),
    ];
}
