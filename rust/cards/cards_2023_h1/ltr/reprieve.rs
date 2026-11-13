// Generated from the card store by `just card header`: notes go below the end marker.
//! Reprieve {1}{W}
//! Instant
//!
//! Return target spell to its owner's hand.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// The spell leaves the stack without resolving, to its owner's hand ([CR 400.3]): "If an object
/// would go to any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." The costs paid for it, X and kicker included, are not refunded. Then
/// Reprieve's controller draws: an illegal target would have stopped the whole spell ([CR 608.2b]),
/// "If all its targets ... are now illegal, the spell or ability doesn't resolve."
///
/// A copy of a spell returned this way would cease to exist ([CR 704.5e]): "If a copy of a spell is
/// in a zone other than the stack, it ceases to exist." No card here copies a spell yet.
///
/// [CR 400.3]: https://yawgatog.com/resources/magic-rules/#R4003
/// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
/// [CR 704.5e]: https://yawgatog.com/resources/magic-rules/#R7045e
pub static CARD: CardDef = Printed::Reprieve
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&sequence(&[return_to_hand(target()), draw(you(), 1)]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Thieving Skydiver kicked for X = 1 taps three of four Islands. Returned to P0's hand,
        // it never enters, and the mana spent on it, X included, stays spent.
        Scenario::new("Reprieve returns a spell with X to its owner's hand and draws")
            .interaction(&[Card::ThievingSkydiver])
            .battlefield(P0, &[Card::Island; 4])
            .hand(P0, &[Card::ThievingSkydiver, Card::Plains])
            .battlefield(P1, &[Card::Plains, Card::Island])
            .hand(P1, &[Card::Reprieve])
            .library(P1, &[Card::Plains])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: Card::ThievingSkydiver,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(1),
                // P0 has nothing to do with the stack full; the Plains to play keeps the game in
                // P0's main phase once it is empty.
                THEM.casts(Card::Reprieve),
                ME.has(Zone::Hand, Card::ThievingSkydiver, 1),
                ME.has(Zone::Battlefield, Card::ThievingSkydiver, 0),
                ME.tapped(Card::Island, 3),
                THEM.has(Zone::Hand, Card::Plains, 1),
                THEM.has(Zone::Graveyard, Card::Reprieve, 1),
            ]),
        // P1 counters the artifact; P0 returns the Counterspell to P1's hand, and the artifact
        // resolves.
        Scenario::new("Reprieve on a counterspell saves the spell it targets")
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Prop::Artifact, Card::Reprieve])
            .battlefield(P1, &[Card::Island; 2])
            .hand(P1, &[Card::Counterspell])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.passes(),
                THEM.casts(Card::Counterspell),
                ME.casts(Card::Reprieve),
                ME.targets(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Hand, Card::Counterspell, 1),
                ME.has(Zone::Hand, Card::Island, 1),
                Resolve,
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
            ]),
    ];
}
