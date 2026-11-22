// Generated from the card store by `just card header`: notes go below the end marker.
//! Sundial of the Infinite {2}
//! Artifact
//!
//! {1}, {T}: End the turn. Activate only during your turn. (Exile all spells and abilities from the
//! stack. Discard down to your maximum hand size. Damage wears off, and "this turn" and "until end
//! of turn" effects end.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SundialOfTheInfinite
    .abilities(&[
        Ability::activated(Cost::of(ManaCost::generic(1), &[CostPart::TAP]))
            .effect(&end_turn())
            .during_your_turn(),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SUNDIAL: &str = Card::SundialOfTheInfinite;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Sundial ends the turn in response to an upkeep trigger")
            .interaction(&[Card::AbhorrentOculus])
            .at(2, P1, TurnStep::End)
            .battlefield(P0, &[SUNDIAL, Card::Island])
            .battlefield(P1, &[Card::AbhorrentOculus])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                stack(1),
                ME.activates(SUNDIAL, 0),
                stack(2),
                Resolve,
                stack(0),
                ME.life(20),
                ME.controls(SUNDIAL, 1),
                ME.tapped(SUNDIAL, 1),
                ME.tapped(Card::Island, 1),
                advance_to(4, TurnStep::PrecombatMain),
            ]),
        Scenario::new("Sundial is unavailable on the opponent's turn")
            .interaction(&[Card::LightningBolt])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[SUNDIAL, Card::Mountain])
            .hand(P0, &[Card::LightningBolt])
            .hand(P1, &[Card::Plains])
            .actions(&[THEM.passes(), ME.deciding(DecisionKind::Priority, 2)]),
        Scenario::new("Sundial skips damage after blockers and the combat boost expires")
            .interaction(&[Card::CoordinatedAssault])
            .hand(P0, &[Card::CoordinatedAssault])
            .battlefield(P0, &[SUNDIAL, Prop::Vanilla, Card::Mountain, Card::Island])
            .battlefield(P1, &[Card::SavannahLions])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                ME.passes(),
                THEM.chooses(DecisionKind::Blocker, Card::SavannahLions),
                ME.casts(Card::CoordinatedAssault),
                ME.target_count(1),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.pt(Prop::Vanilla, 3, 1),
                ME.activates(SUNDIAL, 0),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                ME.attacking(Prop::Vanilla, 0),
                ME.pt(Prop::Vanilla, 2, 1),
                THEM.controls(Card::SavannahLions, 1),
                THEM.life(20),
            ]),
        Scenario::new("Sundial leaves Flickerwisp's delayed trigger for the next turn")
            .interaction(&[Card::Flickerwisp])
            .hand(P0, &[Card::Flickerwisp])
            .battlefield(
                P0,
                &[
                    SUNDIAL,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                ME.activates(SUNDIAL, 0),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                advance_to(2, TurnStep::End),
                Resolve,
                THEM.controls(Prop::Vanilla, 1),
            ]),
    ];
}
