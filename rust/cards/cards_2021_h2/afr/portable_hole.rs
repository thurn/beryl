// Generated from the card store by `just card header`: notes go below the end marker.
//! Portable Hole {W}
//! Artifact
//!
//! When this artifact enters, exile target nonland permanent an opponent controls with mana value 2
//! or less until this artifact leaves the battlefield.
// End of the generated card header.

use crate::prelude::*;

/// "Exile target ... until this artifact leaves the battlefield" ([CR 610.3]): "A second one-shot
/// effect is created immediately after the specified event. This second one-shot effect returns the
/// object to its previous zone." A target gone, or no longer matching, by resolution stops the
/// ability ([CR 608.2b]); a Hole gone first exiles nothing ([CR 610.3b]).
///
/// [CR 610.3]: https://yawgatog.com/resources/magic-rules/#R6103
/// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
/// [CR 610.3b]: https://yawgatog.com/resources/magic-rules/#R6103b
pub static CARD: CardDef = Printed::PortableHole
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::permanent(Filter::All(&[
            Filter::Not(&Filter::Types(Types::LAND)),
            Filter::Whose(&Players::EachOpponent),
            Filter::ManaValueAtMost(2),
        ])))
        .effect(&exile_until_this_leaves(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HOLE: &str = Card::PortableHole;

    /// Mana value 2: a legal target.
    const TWO_DROP: &str = Prop::SmallCreature;

    /// Mana value 3: not a legal target.
    const THREE_DROP: &str = Prop::Creature;

    pub const SCENARIOS: &[Scenario] = &[
        // P1's two-drop is the one legal target: not their three-drop, not P0's own two-drop. On
        // P1's turn Sink into Stupor returns the Hole to P0's hand, and the two-drop comes back
        // under P1's control.
        Scenario::new("Portable Hole exiles until it leaves the battlefield")
            // by-8g5: Playground keeps the Hole in slot zero, exercising linked-exile UI.
            .seed(12)
            .battlefield(P0, &[Card::Plains])
            .permanents(P0, &[Permanent::new(TWO_DROP).sick()])
            .hand(P0, &[HOLE])
            .library(P0, &[Card::Island])
            .battlefield(
                P1,
                &[
                    TWO_DROP,
                    THREE_DROP,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P1, &[Prop::Removal])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(HOLE),
                Resolve,
                Resolve,
                THEM.has(Zone::Exile, TWO_DROP, 1),
                THEM.controls(THREE_DROP, 1),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Prop::Removal),
                THEM.targets(HOLE),
                Resolve,
                ME.has(Zone::Hand, HOLE, 1),
                THEM.has(Zone::Exile, TWO_DROP, 0),
                THEM.controls(TWO_DROP, 1),
            ]),
        // The three-drop's mana value is 3, and P0's own two-drop isn't an opponent's: with no
        // legal target the trigger never goes on the stack.
        Scenario::new("Portable Hole with no permanent of mana value 2 or less to exile")
            .battlefield(P0, &[Card::Plains, TWO_DROP])
            .hand(P0, &[HOLE])
            .library(P0, &[Card::Island])
            .battlefield(P1, &[THREE_DROP])
            .actions(&[
                ME.casts(HOLE),
                Resolve,
                stack(0),
                ME.controls(HOLE, 1),
                THEM.controls(THREE_DROP, 1),
                ME.controls(TWO_DROP, 1),
            ]),
        // With the trigger on the stack, P0 returns its target to P1's hand with Sink into
        // Stupor: the trigger does nothing, and the Hole stays.
        Scenario::new("Portable Hole's target gone before the trigger resolves")
            .battlefield(
                P0,
                &[Card::Plains, Card::Island, Card::Island, Card::Island],
            )
            .hand(P0, &[HOLE, Prop::Removal])
            .library(P0, &[Card::Island])
            .battlefield(P1, &[TWO_DROP])
            .actions(&[
                ME.casts(HOLE),
                Resolve,
                stack(1),
                ME.casts(Prop::Removal),
                Resolve,
                THEM.has(Zone::Hand, TWO_DROP, 1),
                Resolve,
                stack(0),
                THEM.has(Zone::Exile, TWO_DROP, 0),
                ME.controls(HOLE, 1),
            ]),
        // P1 answers the trigger by returning the Hole to P0's hand with Sink into Stupor: the
        // event the exile lasts until has happened, so the two-drop never leaves (CR 610.3b).
        // P0's two-drop in hand, with flash and the Islands for it, gives P0 a priority to pass
        // between the two resolutions.
        Scenario::new("Portable Hole gone before its trigger resolves")
            .battlefield(P0, &[Card::Plains, Card::Island, Card::Island])
            .hand(P0, &[HOLE, TWO_DROP])
            .library(P0, &[Card::Island])
            .battlefield(P1, &[TWO_DROP, Card::Island, Card::Island, Card::Island])
            .hand(P1, &[Card::SinkIntoStupor])
            .actions(&[
                ME.casts(HOLE),
                Resolve,
                stack(1),
                ME.passes(),
                THEM.casts(Card::SinkIntoStupor),
                Resolve,
                Resolve,
                stack(0),
                ME.has(Zone::Hand, HOLE, 1),
                THEM.has(Zone::Exile, TWO_DROP, 0),
                THEM.controls(TWO_DROP, 1),
            ]),
    ];
}
