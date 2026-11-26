// Generated from the card store by `just card header`: notes go below the end marker.
//! Day's Undoing {2}{U}
//! Sorcery
//!
//! Each player shuffles their hand and graveyard into their library, then draws seven cards. If
//! it's your turn, end the turn. (Exile all spells and abilities from the stack, including this
//! card. Discard down to your maximum hand size. Damage wears off, and "this turn" and "until end
//! of turn" effects end.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::DaysUndoing
    .abilities(&[Ability::spell().effect(&sequence(&[
        shuffle_into(
            each_player(),
            Objects::CardsIn {
                players: &each_player(),
                zones: &[Zone::Hand, Zone::Graveyard],
            },
        ),
        draw(each_player(), 7),
        if_then(Condition::TurnOf(you()), &end_turn()),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const UNDOING: &str = Card::DaysUndoing;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "Day's Undoing cast on an opponent's turn goes to the graveyard and does not end the turn",
        )
        .interaction(&[Card::TeferiTimeRaveler])
        .hand(P0, &[UNDOING, Card::Plains])
        .graveyard(P0, &[Card::Swamp])
        .library(P0, &[Card::Island; 8])
        .battlefield(
            P0,
            &[
                Card::TeferiTimeRaveler,
                Card::Island,
                Card::Island,
                Card::Island,
            ],
        )
        .hand(P1, &[Card::Mountain])
        .graveyard(P1, &[Card::Forest])
        .library(P1, &[Card::Mountain; 8])
        .actions(&[
            ME.activates(Card::TeferiTimeRaveler, 1),
            Resolve,
            advance_to(2, TurnStep::PrecombatMain),
            THEM.passes(),
            ME.casts(UNDOING),
            ME.has(Zone::Graveyard, UNDOING, 1),
            ME.has(Zone::Exile, UNDOING, 0),
            ME.has_cards(Zone::Hand, 7),
            THEM.has_cards(Zone::Hand, 7),
            THEM.has_cards(Zone::Graveyard, 0),
            advance_to(2, TurnStep::PostcombatMain),
        ]),
        Scenario::new("Draw triggers from Day's Undoing cease to exist when the turn ends")
            .interaction(&[Card::WizardClass])
            .hand(P0, &[UNDOING])
            .library(P0, &[Card::Island; 10])
            .battlefield(
                P0,
                &[
                    Card::WizardClass,
                    Prop::Vanilla,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P1, &[Card::Mountain; 10])
            .actions(&[
                ME.activates(Card::WizardClass, 1),
                Resolve,
                Resolve,
                ME.activates(Card::WizardClass, 3),
                Resolve,
                ME.casts(UNDOING),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                stack(0),
                ME.has(Zone::Exile, UNDOING, 1),
                ME.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Hand, 8),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 0),
            ]),
    ];
}
