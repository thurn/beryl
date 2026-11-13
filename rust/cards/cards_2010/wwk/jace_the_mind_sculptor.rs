// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, the Mind Sculptor {2}{U}{U}
//! Legendary Planeswalker — Jace, loyalty 3
//!
//! +2: Look at the top card of target player's library. You may put that card on the bottom of that
//! player's library.
//! 0: Draw three cards, then put two cards from your hand on top of your library in any order.
//! −1: Return target creature to its owner's hand.
//! −12: Exile all cards from target player's library, then that player shuffles their hand into
//! their library.
// End of the generated card header.

use crate::prelude::*;

/// "+2: Look at the top card of target player's library. You may put that card on the bottom of
/// that player's library." Looking shows the card to Jace's controller alone, an exception to
/// [CR 401.2]: "Players can't look at or change the order of cards in a library." Moving it within
/// the library is no zone change, so it stays the same object.
///
/// "0: Draw three cards, then put two cards from your hand on top of your library in any order."
/// Each card chosen goes on top of the one before, so the second is the top card. [CR 121.2]: "If a
/// player is instructed to draw multiple cards, that player performs that many individual card
/// draws."
///
/// "−1: Return target creature to its owner's hand." [CR 400.3]: "If an object would go to any
/// library, graveyard, or hand other than its owner's, it goes to its owner's corresponding zone."
///
/// "−12: Exile all cards from target player's library, then that player shuffles their hand into
/// their library." [CR 701.13a]: "To exile an object, move it to the exile zone from wherever it
/// is." Then [CR 701.24a]: "To shuffle a library ... randomize the cards within it so that no
/// player knows their order."
///
/// [CR 401.2]: https://yawgatog.com/resources/magic-rules/#R4012
/// [CR 121.2]: https://yawgatog.com/resources/magic-rules/#R1212
/// [CR 400.3]: https://yawgatog.com/resources/magic-rules/#R4003
/// [CR 701.13a]: https://yawgatog.com/resources/magic-rules/#R70113a
/// [CR 701.24a]: https://yawgatog.com/resources/magic-rules/#R70124a
pub static CARD: CardDef = Printed::JaceTheMindSculptor
    .abilities(&[
        Ability::loyalty(2)
            .target(&TargetSpec::player())
            .effect(&fateseal(you(), target_player())),
        Ability::loyalty(0).effect(&sequence(&[draw(you(), 3), put_back(you(), 2)])),
        Ability::loyalty(-1)
            .target(&TargetSpec::creature())
            .effect(&return_to_hand(target())),
        Ability::loyalty(-12)
            .target(&TargetSpec::player())
            .effect(&sequence(&[
                exile(library_of(&target_player())),
                shuffle_into(target_player(), hand_of(&target_player())),
            ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const JACE: &str = Card::JaceTheMindSculptor;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("+2 puts the top of an opponent's library on the bottom")
            .battlefield(P0, &[JACE])
            .library(P0, &[Card::Island, Card::Plains])
            .library(P1, &[Prop::Artifact, Card::Plains, Card::Island])
            .actions(&[
                ME.activates(JACE, 0), // +2
                ME.targets_player(THEM),
                Resolve,
                ME.yes(),
                // Assertions read the next open decision: by then, the opponent has drawn the
                // Plains that the artifact left on top.
                THEM.library_top(&[Card::Island, Prop::Artifact]),
                THEM.has(Zone::Hand, Card::Plains, 1),
                ME.has_loyalty(JACE, 5),
                // The next turn, Jace looks at his own top card and keeps it there.
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(JACE, 0),
                ME.targets_player(ME),
                ME.no(),
                ME.library_top(&[Card::Plains]),
                ME.has_loyalty(JACE, 7),
            ]),
        Scenario::new("0 draws three and puts two back in the order chosen")
            .battlefield(P0, &[JACE])
            .hand(P0, &[Prop::Artifact])
            .library(
                P0,
                &[Card::Island, Card::Plains, Prop::Creature, Card::Island],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(JACE, 1), // 0
                Resolve,
                ME.picks(Prop::Artifact),
                ME.picks(Card::Island),
                ME.library_top(&[Card::Island, Prop::Artifact, Card::Island]),
                ME.has_cards(Zone::Hand, 2),
                ME.has_loyalty(JACE, 3),
            ]),
        Scenario::new("−1 returns a creature; −12 exiles a library and shuffles in the hand")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 13)],
            )
            .battlefield(P1, &[Prop::Creature])
            .hand(P1, &[Card::Plains])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                ME.activates(JACE, 2), // −1
                Resolve,
                THEM.has(Zone::Hand, Prop::Creature, 1),
                ME.has_loyalty(JACE, 12),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(JACE, 3), // −12
                ME.targets_player(THEM),
                // Turn 2's draw left two Islands, now exiled; the hand of three is the library.
                THEM.has(Zone::Exile, Card::Island, 2),
                THEM.has_cards(Zone::Library, 3),
                THEM.has_cards(Zone::Hand, 0),
                // Loyalty 0: put into its owner's graveyard (CR 704.5i).
                ME.has(Zone::Graveyard, JACE, 1),
            ]),
    ];
}
