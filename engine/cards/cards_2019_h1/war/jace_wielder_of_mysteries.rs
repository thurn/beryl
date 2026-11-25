// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, Wielder of Mysteries {1}{U}{U}{U}
//! Legendary Planeswalker — Jace, loyalty 4
//!
//! If you would draw a card while your library has no cards in it, you win the game instead.
//! +1: Target player mills two cards. Draw a card.
//! −8: Draw seven cards. Then if your library has no cards in it, you win the game.
// End of the generated card header.

use crate::prelude::*;

const LIBRARY_IS_EMPTY: Condition = Condition::Compare(
    Amount::Count(&Objects::All {
        zone: Zone::Library,
        filter: yours(),
    }),
    Comparison::Equal,
    Amount::Fixed(0),
);

static YOU_WIN: Effect = win_game(you());

/// Jace can spend his last eight loyalty on the −8. The loyalty cost leaves him at zero,
/// so [CR 704.5i] puts him “into its owner's graveyard.” The second sentence of the ability
/// can still win before [CR 704.5b] checks whether his controller “attempted to draw a
/// card from a library with no cards in it.”
///
/// [CR 704.5i]: https://yawgatog.com/resources/magic-rules/#R7045i
/// [CR 704.5b]: https://yawgatog.com/resources/magic-rules/#R7045b
pub static CARD: CardDef = Printed::JaceWielderOfMysteries
    .abilities(&[
        Ability::replacing(&Replacement::draws(Players::You))
            .only_if(&LIBRARY_IS_EMPTY)
            .effect(&YOU_WIN),
        Ability::loyalty(1)
            .target(&TargetSpec::player())
            .effect(&sequence(&[mill(target_player(), 2), draw(you(), 1)])),
        Ability::loyalty(-8).effect(&sequence(&[
            draw(you(), 7),
            if_then(LIBRARY_IS_EMPTY, &YOU_WIN),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const JACE: &str = Card::JaceWielderOfMysteries;

    /// The lands to flash in `Prop::FlashCreature` with.
    const FLASH_MANA: &[&str] = &[Card::Swamp, Card::Island, Card::Island];

    const FLASH_MANA_AND_JACE: &[&str] = &[JACE, Card::Swamp, Card::Island, Card::Island];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("+1 mills the target player two, then draws")
            .battlefield(P0, &[JACE])
            .hand(P0, &[Card::Plains])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Plains, Card::Plains, Card::Island])
            .actions(&[
                ME.activates(JACE, 1), // +1
                ME.targets_player(THEM),
                Resolve,
                THEM.has_cards(Zone::Graveyard, 2),
                THEM.library_top(&[Card::Island]),
                ME.has_cards(Zone::Hand, 2),
                ME.has_loyalty(JACE, 5),
            ]),
        // Milling his own last two cards, Jace draws from the empty library: a win, not a loss.
        Scenario::new("+1 milling his controller empty wins on the draw")
            .battlefield(P0, &[JACE])
            .library(P0, &[Card::Island, Card::Plains])
            .actions(&[
                ME.activates(JACE, 1), // +1
                // P0 has nothing else to do, so the ability resolves at once.
                ME.targets_player(ME),
                ME.has_cards(Zone::Graveyard, 2),
                wins(ME),
            ]),
        // A creature with flash gives P0 something to pass in the upkeep.
        Scenario::new("the draw step's draw from an empty library wins")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA_AND_JACE)
            .hand(P0, &[Prop::FlashCreature])
            .library(P0, &[])
            .actions(&[ME.passes(), wins(ME)]),
        // The replacement is Jace's controller's alone: the opponent drawing from an empty
        // library still loses (CR 704.5b).
        Scenario::new("the opponent's draw from an empty library still loses")
            .at(2, P1, TurnStep::Upkeep)
            .battlefield(P0, &[JACE])
            .battlefield(P1, FLASH_MANA)
            .hand(P1, &[Prop::FlashCreature])
            .library(P1, &[])
            .actions(&[THEM.passes(), wins(ME)]),
        // At exactly eight loyalty Jace dies before the ability resolves: no replacement. Five
        // cards are drawn, two draws fail, and the library is empty: a win.
        Scenario::new("−8 draws the library empty and wins")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 8)],
            )
            .library(
                P0,
                &[
                    Card::Island,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.activates(JACE, 2), // −8
                wins(ME),
                ME.has_cards(Zone::Hand, 5),
            ]),
        // With cards left after the seven, there is no win.
        Scenario::new("−8 with cards left after the seven doesn't win")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 9)],
            )
            .library(
                P0,
                &[
                    Card::Island,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                ],
            )
            .actions(&[
                ME.activates(JACE, 2), // −8
                Resolve,
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 1),
                ME.has_loyalty(JACE, 1),
            ]),
    ];
}
