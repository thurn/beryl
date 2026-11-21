// Generated from the card store by `just card header`: notes go below the end marker.
//! Griselbrand {4}{B}{B}{B}{B}
//! Legendary Creature — Demon, 7/7
//!
//! Flying, lifelink
//! Pay 7 life: Draw seven cards.
// End of the generated card header.

use crate::prelude::*;

/// A player at exactly 7 life can pay the cost ([CR 119.4]) and loses before drawing any
/// cards, when state-based actions see their life total of zero ([CR 704.5a]).
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
/// [CR 704.5a]: https://yawgatog.com/resources/magic-rules/#R7045a
pub static CARD: CardDef = Printed::Griselbrand
    .abilities(&[Ability::activated(Cost::part(&CostPart::Life(7))).effect(&draw(you(), 7))])
    .keywords(Keywords::FLYING.union(Keywords::LIFELINK))
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GRISELBRAND: &str = Card::Griselbrand;

    const LIBRARY: &[&str] = &[Card::Island; 8];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("paying 7 life draws seven cards")
            .battlefield(P0, &[GRISELBRAND])
            .life(P0, 20)
            .library(P0, LIBRARY)
            .actions(&[
                ME.activates(GRISELBRAND, 0),
                Resolve,
                ME.life(13),
                ME.has_cards(Zone::Hand, 7),
            ]),
        // [CR 119.4]: a life total equal to the payment may pay it.
        Scenario::new("exactly 7 life may pay, leaving 0")
            .battlefield(P0, &[GRISELBRAND])
            .life(P0, 7)
            .library(P0, LIBRARY)
            .actions(&[
                ME.deciding(DecisionKind::Priority, 2),
                ME.activates(GRISELBRAND, 0),
                ME.life(0),
            ]),
        // Only passing is offered at 6 life, so the game gives no priority and goes on to
        // declare attackers: the ruling "You can't activate Griselbrand's ability if you have 6
        // or less life."
        Scenario::new("6 life cannot pay")
            .battlefield(P0, &[GRISELBRAND])
            .life(P0, 6)
            .library(P0, LIBRARY)
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
    ];
}
