// Generated from the card store by `just card header`: notes go below the end marker.
//! Worldgorger Dragon {3}{R}{R}{R}
//! Creature — Nightmare Dragon, 7/7
//!
//! Flying, trample
//! When this creature enters, exile all other permanents you control.
//! When this creature leaves the battlefield, return the exiled cards to the battlefield under
//! their owners' control.
// End of the generated card header.

use crate::prelude::*;

/// The enters ability exiles every other permanent its controller controls, linked to this
/// Dragon ([CR 607.2a]): "If an object has an activated or triggered ability printed on it that
/// instructs a player to exile one or more cards and an ability printed on it that refers either
/// to "the exiled cards" or to cards "exiled with [this object]," these abilities are linked."
///
/// The leaves ability looks back in time ([CR 603.10a]) and returns the cards exiled with the
/// Dragon under their owners' control ([CR 110.2a]), each as a new object ([CR 400.7]), so a land
/// that was tapped comes back untapped. An Aura among them chooses what it enchants as it
/// returns and targets nothing ([CR 303.4f]).
///
/// With Animate Dead the two abilities make a loop: Animate Dead returns the Dragon, the Dragon
/// exiles Animate Dead, Animate Dead's leaves ability sacrifices the Dragon, and the Dragon's
/// leaves ability returns Animate Dead, which must enchant a creature card in a graveyard. With
/// the Dragon the only one, nobody can stop it, and the game is a draw ([CR 104.4b]).
///
/// [CR 607.2a]: https://yawgatog.com/resources/magic-rules/#R6072a
/// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
/// [CR 110.2a]: https://yawgatog.com/resources/magic-rules/#R1102a
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
/// [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
/// [CR 104.4b]: https://yawgatog.com/resources/magic-rules/#R1044b
pub static CARD: CardDef = Printed::WorldgorgerDragon
    .keywords(Keywords::FLYING.union(Keywords::TRAMPLE))
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS).effect(&exile_linked(Objects::Other(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Whose(&Players::You),
        }))),
        Ability::triggered(&Trigger::LEAVES).effect(&return_to_battlefield(exiled_with_this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DRAGON: &str = Card::WorldgorgerDragon;
    const ANIMATE: &str = Card::AnimateDead;

    /// Animate Dead in P0's hand with a Swamp and a Plains to cast it, and the Dragon in P0's
    /// graveyard.
    const LOOP: Scenario = Scenario::setup()
        .interaction(&[ANIMATE])
        .hand(P0, &[ANIMATE])
        .battlefield(P0, &[Card::Swamp, Card::Plains])
        .graveyard(P0, &[DRAGON])
        .library(P0, &[Card::Plains])
        .library(P1, &[Card::Plains]);

    pub const SCENARIOS: &[Scenario] = &[
        // Animate Dead returns the Dragon, which exiles Animate Dead and the lands; Animate
        // Dead leaving makes P0 sacrifice the Dragon, whose leaves ability returns them all;
        // Animate Dead, with the Dragon the only creature card in a graveyard, must enchant it
        // and return it again. No player can stop the loop, so the game is a draw (CR 104.4b).
        LOOP.titled("Animate Dead on Worldgorger Dragon alone is a draw")
            // Each time around is four resolutions. The engine takes its first hash at the ninth
            // resolution with no choice, and finds the same position four later.
            .actions(&[
                ME.casts(ANIMATE),
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                Resolve,
                ends_in_draw(),
            ]),
        // With P1's creature card in a graveyard too, Animate Dead returning from exile may
        // enchant it instead of the Dragon (CR 303.4f). That choice is optional, so the loop
        // is not a draw, and choosing the other card ends it with the Dragon in the graveyard.
        LOOP.titled("A second creature card lets Animate Dead end the loop")
            .graveyard(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(ANIMATE),
                ME.targets(DRAGON),
                Resolve,
                Resolve,
                ME.controls(DRAGON, 1),
                Resolve,
                ME.has(Zone::Exile, ANIMATE, 1),
                ME.has(Zone::Exile, Card::Swamp, 1),
                Resolve,
                Resolve,
                ME.chooses(DecisionKind::ChooseObject, Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.controls(ANIMATE, 1),
                ME.controls(Card::Swamp, 1),
                ME.has(Zone::Graveyard, DRAGON, 1),
            ]),
        // The Dragon exiles P0's lands, tapped to cast it, and leaves P1's creature alone. P1
        // destroys it, and the lands come back untapped.
        Scenario::new("Worldgorger Dragon exiles your permanents until it leaves")
            .interaction(&[Card::GetLost])
            .hand(P0, &[DRAGON])
            .battlefield(P0, &[Card::Mountain; 6])
            // A second Get Lost and the mana for it give P1 something to do with each
            // priority, so the leaves ability waits for a pass.
            .hand(P1, &[Card::GetLost, Card::GetLost])
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Vanilla,
                ],
            )
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(DRAGON),
                Resolve,
                ME.tapped(Card::Mountain, 6),
                Resolve,
                ME.controls(Card::Mountain, 0),
                ME.has(Zone::Exile, Card::Mountain, 6),
                THEM.controls(Prop::Vanilla, 1),
                ME.passes(),
                THEM.casts(Card::GetLost),
                THEM.targets(DRAGON),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, DRAGON, 1),
                ME.controls(Card::Mountain, 6),
                ME.tapped(Card::Mountain, 0),
                ME.has(Zone::Exile, Card::Mountain, 0),
            ]),
    ];
}
