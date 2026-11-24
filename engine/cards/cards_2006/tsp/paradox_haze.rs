// Generated from the card store by `just card header`: notes go below the end marker.
//! Paradox Haze {2}{U}
//! Enchantment — Aura
//!
//! Enchant player
//! At the beginning of enchanted player's first upkeep each turn, that player gets an additional
//! upkeep step after this step.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ParadoxHaze
    .abilities(&[
        Ability::spell().target(&TargetSpec::player()),
        Ability::triggered(&Trigger::first_step(TurnStep::Upkeep, Players::Enchanted))
            .effect(&Effect::AdditionalStep(TurnStep::Upkeep)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HAZE: &str = Card::ParadoxHaze;
    const CRYPT: &str = Card::ManaCrypt;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the other player gets two upkeeps and a trigger in each")
            .interaction(&[CRYPT])
            .hand(P0, &[HAZE])
            .battlefield(P0, &[Card::Island; 3])
            .battlefield(P1, &[CRYPT])
            .actions(&[
                ME.casts(HAZE),
                ME.targets_player(THEM),
                Resolve,
                ME.controls(HAZE, 1),
                advance_to(2, TurnStep::Upkeep),
                stack(2),
                Resolve,
                Resolve,
                stack(0),
                THEM.passes(),
                stack(1),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.has_cards(Zone::Hand, 1),
            ]),
        Scenario::new("two Auras give one player three upkeeps")
            .interaction(&[CRYPT])
            .hand(P0, &[HAZE; 2])
            .battlefield(P0, &[Card::Island; 6])
            .battlefield(P1, &[CRYPT])
            .actions(&[
                ME.casts(HAZE),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(HAZE),
                ME.targets_player(THEM),
                Resolve,
                ME.controls(HAZE, 2),
                advance_to(2, TurnStep::Upkeep),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.passes(),
                stack(1),
                Resolve,
                THEM.passes(),
                stack(1),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.has_cards(Zone::Hand, 1),
            ]),
        Scenario::new("protection from everything sends the attached Aura to the graveyard")
            .interaction(&[Card::TeferisProtection])
            .hand(P0, &[HAZE])
            .hand(P1, &[Card::TeferisProtection])
            .battlefield(P0, &[Card::Island; 3])
            .battlefield(P1, &[Card::Plains; 3])
            .actions(&[
                ME.casts(HAZE),
                ME.targets_player(THEM),
                Resolve,
                ME.controls(HAZE, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::TeferisProtection),
                Resolve,
                ME.has(Zone::Graveyard, HAZE, 1),
            ]),
    ];
}
