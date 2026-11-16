// Generated from the card store by `just card header`: notes go below the end marker.
//! Builder's Blessing {3}{W}
//! Enchantment
//!
//! Untapped creatures you control get +0/+2.
// End of the generated card header.

use crate::prelude::*;

/// "Untapped creatures you control get +0/+2" is a layer 7c modification of every untapped
/// creature its controller controls ([CR 613.4c]): "Effects and counters that modify power
/// and/or toughness (but don't set power and/or toughness to a specific number or value) are
/// applied." Whether a creature is tapped is no characteristic that a layer changes, so the
/// set is read as the creatures' tapped status stands: a creature that taps (to attack, say)
/// stops getting the bonus, and one that untaps gets it again. The controller is read for
/// the permanent with the ability ([CR 109.4]): "Only objects on the stack or on the
/// battlefield have a controller."
///
/// [CR 109.4]: https://yawgatog.com/resources/magic-rules/#R1094
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::BuildersBlessing
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[Filter::Types(Types::CREATURE), YOU_CONTROL, UNTAPPED]),
        },
        condition: None,
        modifications: &[Modification::ModifyPt(0, 2)],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BLESSING: &str = Card::BuildersBlessing;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("an untapped creature of its controller gets +0/+2")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Flier,
                ],
            )
            .hand(P0, &[BLESSING])
            .actions(&[
                ME.pt(Prop::Flier, 3, 1),
                ME.casts(BLESSING),
                Resolve,
                ME.pt(Prop::Flier, 3, 3),
            ]),
        // Attacking taps the flier, which then loses the bonus.
        Scenario::new("a tapped creature does not get it")
            .battlefield(P0, &[BLESSING, Prop::Flier])
            .actions(&[
                ME.pt(Prop::Flier, 3, 3),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Flier),
                ME.tapped(Prop::Flier, 1),
                ME.pt(Prop::Flier, 3, 1),
            ]),
        Scenario::new("a creature an opponent controls does not get it")
            .battlefield(P0, &[BLESSING])
            .hand(P0, &[Card::Plains])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[THEM.pt(Prop::Flier, 3, 1)]),
    ];
}
