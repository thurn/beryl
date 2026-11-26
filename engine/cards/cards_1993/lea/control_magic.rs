// Generated from the card store by `just card header`: notes go below the end marker.
//! Control Magic {2}{U}{U}
//! Enchantment — Aura
//!
//! Enchant creature
//! You control enchanted creature.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ControlMagic
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::Control],
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MAGIC: &str = Card::ControlMagic;

    pub const SCENARIOS: &[Scenario] = &[
        // A creature P0 gains control of has not been under P0's control since P0's turn
        // began, so it can't attack this turn; the flier P0 had can. On P0's next turn the
        // stolen creature attacks too.
        Scenario::new("Control Magic's creature can't attack until its new controller's turn")
            .hand(P0, &[MAGIC])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Flier,
                ],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(MAGIC),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                THEM.controls(Prop::Vanilla, 0),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 2),
                ME.done_attacking(),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 3),
            ]),
        // When Control Magic leaves the battlefield, the creature goes back to its owner. P1's
        // Metamorphose puts the Aura on top of P0's library, and P1 controls the creature again
        // and may put a card from their hand.
        Scenario::new("the creature returns when Control Magic leaves")
            .interaction(&[Card::Metamorphose])
            .hand(P0, &[MAGIC])
            .battlefield(P0, &[Card::Island; 4])
            .hand(P1, &[Card::Metamorphose])
            .battlefield(P1, &[Card::Island, Card::Island, Prop::Vanilla])
            .actions(&[
                ME.casts(MAGIC),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.passes(),
                THEM.casts(Card::Metamorphose),
                THEM.targets(MAGIC),
                ME.library_top(&[MAGIC]),
                THEM.controls(Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
            ]),
        // The ruling: "Gaining control of a creature doesn't cause you to gain control of any
        // Auras or Equipment attached to it." P1's Holy Strength stays P1's and still gives
        // the creature +1/+2 under P0's control.
        Scenario::new("Control Magic doesn't take the Auras on the creature")
            .interaction(&[Card::HolyStrength])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P1, &[Card::HolyStrength])
            .battlefield(P1, &[Card::Plains, Prop::Vanilla])
            .hand(P0, &[MAGIC])
            .battlefield(P0, &[Card::Island; 4])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                THEM.casts(Card::HolyStrength),
                Resolve,
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(MAGIC),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                THEM.controls(Card::HolyStrength, 1),
                ME.pt(Prop::Vanilla, 3, 3),
            ]),
    ];
}
