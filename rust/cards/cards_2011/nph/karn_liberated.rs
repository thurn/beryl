// Generated from the card store by `just card header`: notes go below the end marker.
//! Karn Liberated {7}
//! Legendary Planeswalker — Karn, loyalty 6
//!
//! +4: Target player exiles a card from their hand.
//! −3: Exile target permanent.
//! −14: Restart the game, leaving in exile all non-Aura permanent cards exiled with Karn. Then put
//! those cards onto the battlefield under your control.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::KarnLiberated
    .abilities(&[
        Ability::loyalty(4)
            .target(&TargetSpec::player())
            .effect(&sequence(&[
                choose(
                    target_player(),
                    hand_of(&target_player()),
                    Prompt::new(Purpose::Choose)
                        .most(1)
                        .places(Place::Exile, Place::None),
                ),
                exile_linked(those()),
            ])),
        Ability::loyalty(-3)
            .target(&TargetSpec::permanent(Filter::Any))
            .effect(&exile_linked(target())),
        Ability::loyalty(-14).effect(&restart_game(Filter::All(&[
            Filter::Types(Types::PERMANENT),
            Filter::Not(&Filter::Subtype(Subtype::Aura)),
        ]))),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::FOOD_TOKEN;

    const KARN: &str = Card::KarnLiberated;
    const BOG: &str = Card::BojukaBog;
    const AURA: &str = Card::HolyStrength;
    const INSTANT: &str = Prop::Instant;
    const CREATURE: &str = Prop::Vanilla;
    const ARTIFACT: &str = Prop::Equipment;
    const KEEP: DecisionKind = DecisionKind::KeepOrMulligan;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Karn restarts with the linked non-Aura permanent cards")
            .at(4, P1, TurnStep::PrecombatMain)
            .life(P0, 8)
            .life(P1, 13)
            .interaction(&[BOG, AURA, Card::OkoThiefOfCrowns])
            .graveyard(P1, &[Card::OkoThiefOfCrowns])
            .permanents(P0, &[Permanent::token(FOOD_TOKEN)])
            .permanents(
                P1,
                &[Permanent::new(KARN)
                    .counters(CounterKind::Loyalty, 12)
                    .exiled_with(&[AURA, CREATURE])],
            )
            .battlefield(P1, &[ARTIFACT])
            .hand(P0, &[BOG, Card::Plains])
            .hand(P1, &[INSTANT, Card::Island])
            .library(P0, &[Card::Plains; 10])
            .library(P1, &[Card::Island; 10])
            .actions(&[
                THEM.activates(KARN, 0),
                THEM.targets_player(ME),
                Resolve,
                ME.chooses(DecisionKind::ChooseObject, BOG),
                ME.has(Zone::Exile, BOG, 1),
                THEM.has_loyalty(KARN, 16),
                advance_to(6, TurnStep::PrecombatMain),
                THEM.activates(KARN, 1),
                THEM.targets(ARTIFACT),
                Resolve,
                THEM.has(Zone::Exile, ARTIFACT, 1),
                THEM.has_loyalty(KARN, 13),
                advance_to(8, TurnStep::PrecombatMain),
                THEM.activates(KARN, 1),
                THEM.targets(FOOD_TOKEN),
                Resolve,
                ME.controls(FOOD_TOKEN, 0),
                advance_to(10, TurnStep::PrecombatMain),
                THEM.activates(KARN, 0),
                THEM.targets_player(THEM),
                Resolve,
                THEM.chooses(DecisionKind::ChooseObject, INSTANT),
                THEM.has(Zone::Exile, INSTANT, 1),
                advance_to(12, TurnStep::PrecombatMain),
                THEM.activates(KARN, 2),
                THEM.controls(KARN, 0),
                Resolve,
                stack(0),
                ME.life(20),
                THEM.life(20),
                ME.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Hand, 7),
                ME.has(Zone::Exile, BOG, 1),
                THEM.has(Zone::Exile, CREATURE, 1),
                THEM.has(Zone::Exile, AURA, 0),
                THEM.has(Zone::Exile, INSTANT, 0),
                THEM.answers(KEEP, Pick::Keep),
                ME.answers(KEEP, Pick::Mulligan),
                ME.has(Zone::Exile, BOG, 1),
                ME.answers(KEEP, Pick::Keep),
                THEM.targets_player(ME),
                advance_to(1, TurnStep::Upkeep),
                stack(1),
                THEM.controls(BOG, 1),
                ME.has(Zone::Battlefield, BOG, 1),
                THEM.controls(CREATURE, 1),
                THEM.controls(ARTIFACT, 1),
                THEM.tapped(BOG, 0),
                ME.controls(FOOD_TOKEN, 0),
                THEM.controls(KARN, 0),
                Resolve,
                advance_to(1, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 6),
                THEM.has_cards(Zone::Hand, 7),
                advance_to(1, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(CREATURE),
            ]),
        Scenario::new("Karn restarts even when a library cannot deal seven cards")
            .permanents(
                P0,
                &[Permanent::new(KARN).counters(CounterKind::Loyalty, 14)],
            )
            .library(P0, &[Card::Plains; 8])
            .library(P1, &[Card::Island; 6])
            .actions(&[
                ME.activates(KARN, 2),
                Resolve,
                ME.answers(KEEP, Pick::Keep),
                THEM.answers(KEEP, Pick::Keep),
                wins(ME),
            ]),
    ];
}
