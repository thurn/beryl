// Generated from the card store by `just card header`: notes go below the end marker.
//! Stifle {U}
//! Instant
//!
//! Counter target activated or triggered ability. (Mana abilities can't be targeted.)
// End of the generated card header.

use crate::prelude::*;

/// Countering an ability removes it from the stack and leaves its source alone ([CR 701.6a]):
/// "To counter a spell or ability means to cancel it, removing it from the stack. It doesn't
/// resolve and none of its effects occur." A mana ability never goes on the stack, so it is never
/// a target ([CR 605.3b]).
///
/// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
/// [CR 605.3b]: https://yawgatog.com/resources/magic-rules/#R6053b
pub static CARD: CardDef = Printed::Stifle
    .abilities(&[Ability::spell()
        .target(&TargetSpec::ability())
        .effect(&counter(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // P1 sacrifices Strip Mine to destroy P0's Plains. Stifle counters the ability: the
        // Plains stays, and Strip Mine, sacrificed as a cost, stays in the graveyard.
        Scenario::new("Stifle counters an activated ability")
            .interaction(&[Card::StripMine])
            .battlefield(P0, &[Card::Island, Card::Plains])
            .hand(P0, &[Card::Stifle])
            .battlefield(P1, &[Card::StripMine])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.activates(Card::StripMine, 1), // {T}, Sacrifice
                THEM.targets(Card::Plains),
                ME.casts(Card::Stifle),
                stack(0),
                ME.controls(Card::Plains, 1),
                THEM.has(Zone::Graveyard, Card::StripMine, 1),
                ME.has(Zone::Graveyard, Card::Stifle, 1),
            ]),
        // Fiend Hunter's enters ability targets P0's creature, and Stifle counters it: the
        // creature is never exiled, and Fiend Hunter stays.
        Scenario::new("Stifle counters an enters trigger")
            .interaction(&[Card::FiendHunter])
            .battlefield(P0, &[Card::Island, Prop::Vanilla])
            .hand(P0, &[Card::Stifle])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P1, &[Card::FiendHunter])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Card::FiendHunter),
                Resolve,
                THEM.passes(),
                ME.casts(Card::Stifle),
                stack(0),
                ME.controls(Prop::Vanilla, 1),
                THEM.controls(Card::FiendHunter, 1),
            ]),
        // A delayed triggered ability is a target too (ruling 2004-10-04). Flickerwisp exiles
        // one of P0's Islands; Stifle counters the end step trigger that would return it, so it
        // stays exiled, and the countered trigger does not wait to trigger again.
        Scenario::new("Stifle counters a delayed trigger")
            .interaction(&[Card::Flickerwisp])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::Stifle])
            .battlefield(P1, &[Card::Plains, Card::Plains, Card::Plains])
            .hand(P1, &[Card::Flickerwisp])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Card::Flickerwisp),
                Resolve,
                THEM.targets(Card::Island),
                Resolve,
                ME.has(Zone::Exile, Card::Island, 1),
                advance_to(2, TurnStep::End),
                stack(1),
                THEM.passes(),
                ME.casts(Card::Stifle),
                stack(0),
                ME.has(Zone::Exile, Card::Island, 1),
                advance_to(3, TurnStep::End),
                ME.has(Zone::Exile, Card::Island, 1),
                ME.controls(Card::Island, 1),
            ]),
        // Two Stifles target Strip Mine's ability. The second counters it, so the first one's
        // only target has left the stack: it is illegal (CR 608.2b), the first Stifle does not
        // resolve, and nothing else happens.
        Scenario::new("Stifle whose target was countered first does not resolve")
            .interaction(&[Card::StripMine])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[Card::Stifle, Card::Stifle])
            .battlefield(P1, &[Card::StripMine])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.activates(Card::StripMine, 1), // {T}, Sacrifice
                THEM.targets(Card::Plains),
                ME.casts(Card::Stifle),
                ME.casts(Card::Stifle),
                stack(0),
                ME.controls(Card::Plains, 1),
                ME.has(Zone::Graveyard, Card::Stifle, 2),
            ]),
        // P1 casts an artifact and then sacrifices Strip Mine to destroy an Island. P0 Stifles
        // the ability and then counters the artifact beneath it. The artifact leaving from below
        // moves the ability down the stack, and Stifle still counters it: the Island stays.
        Scenario::new("Stifle follows its target down the stack")
            .interaction(&[Card::StripMine])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Stifle, Card::Counterspell])
            .battlefield(P1, &[Card::Plains, Card::StripMine])
            .hand(P1, &[Prop::Artifact])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Prop::Artifact),
                THEM.activates(Card::StripMine, 1), // {T}, Sacrifice
                THEM.targets(Card::Island),
                ME.casts(Card::Stifle),
                ME.casts(Card::Counterspell),
                ME.targets(Prop::Artifact),
                stack(0),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.controls(Card::Island, 3),
                ME.has(Zone::Graveyard, Card::Stifle, 1),
            ]),
    ];
}
