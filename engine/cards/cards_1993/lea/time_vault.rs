// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Vault {2}
//! Artifact
//!
//! This artifact enters tapped.
//! This artifact doesn't untap during your untap step.
//! If you would begin your turn while this artifact is tapped, you may skip that turn instead. If
//! you do, untap this artifact.
//! {T}: Take an extra turn after this one.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TimeVault
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::UntapInUntapStep(Objects::This),
            when: When::Never,
        }),
        Ability::replacing(&Replacement::SKIP_TURN_TO_UNTAP),
        Ability::activated(Cost::TAP).effect(&extra_turn(you())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const VAULT: &str = Card::TimeVault;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Time Vault enters tapped and skips a turn to untap")
            .hand(P0, &[VAULT])
            .battlefield(P0, &[Card::Island; 2])
            .library(P0, &[Card::Forest; 2])
            .library(P1, &[Card::Plains; 3])
            .actions(&[
                ME.casts(VAULT),
                Resolve,
                ME.tapped(VAULT, 1),
                advance_to(2, TurnStep::End),
                THEM.passes(),
                ME.picks_up_to(VAULT),
                advance_to(3, TurnStep::PrecombatMain),
                ME.tapped(VAULT, 0),
                THEM.has(Zone::Hand, Card::Plains, 2),
                ME.has(Zone::Hand, Card::Forest, 0),
                THEM.passes(),
                ME.activates(VAULT, 3),
                advance_to(3, TurnStep::End),
                THEM.passes(),
                ME.done_picking(),
                advance_to(4, TurnStep::PrecombatMain),
                ME.has(Zone::Hand, Card::Forest, 1),
                ME.tapped(VAULT, 1),
            ]),
        Scenario::new("Time Vault stays tapped when its controller takes the turn")
            .at(2, P1, TurnStep::End)
            .permanents(P0, &[Permanent::new(VAULT).tapped()])
            .actions(&[
                advance_to(2, TurnStep::End),
                THEM.passes(),
                ME.done_picking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 1),
                ME.tapped(VAULT, 1),
            ]),
        Scenario::new("The Vault's extra turn is taken before the earlier extra turn")
            .interaction(&[Card::TimeWarp])
            .hand(P0, &[Card::TimeWarp])
            .battlefield(
                P0,
                &[
                    VAULT,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P1, &[Card::Plains; 2])
            .actions(&[
                ME.casts(Card::TimeWarp),
                ME.targets_player(THEM),
                Resolve,
                ME.activates(VAULT, 3),
                Resolve,
                advance_to(1, TurnStep::End),
                ME.passes(),
                ME.done_picking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 1),
                THEM.has_cards(Zone::Hand, 0),
                advance_to(3, TurnStep::PrecombatMain),
                THEM.has_cards(Zone::Hand, 1),
                advance_to(4, TurnStep::PrecombatMain),
                THEM.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("Two Vault activations grant turns without requiring skips")
            .battlefield(P0, &[VAULT; 2])
            .library(P0, &[Card::Island; 2])
            .actions(&[
                ME.activates(VAULT, 3),
                Resolve,
                ME.activates(VAULT, 3),
                Resolve,
                advance_to(1, TurnStep::End),
                ME.passes(),
                ME.done_picking(),
                advance_to(2, TurnStep::End),
                ME.passes(),
                ME.done_picking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 2),
                ME.tapped(VAULT, 2),
                advance_to(4, TurnStep::PrecombatMain),
                THEM.has_cards(Zone::Hand, 1),
            ]),
    ];
}
