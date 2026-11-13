// Generated from the card store by `just card header`: notes go below the end marker.
//! Trinket Mage {2}{U}
//! Creature — Human Wizard, 2/2
//!
//! When this creature enters, you may search your library for an artifact card with mana value 1 or
//! less, reveal that card, put it into your hand, then shuffle.
// End of the generated card header.

use crate::prelude::*;

/// "An artifact card with mana value 1 or less".
///
/// "You may" covers the whole instruction, the shuffle with it: declining searches nothing
/// and shuffles nothing. Searching, the player may find nothing
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)) and shuffles all the
/// same ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
/// library ..., randomize the cards within it so that no player knows their order." The card
/// found is revealed ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)):
/// "To reveal a card, show that card to all players for a brief time."
pub static CARD: CardDef = Printed::TrinketMage
    .abilities(&[Ability::triggered(&Trigger::ENTERS).effect(&may(
        you(),
        &search(
            you(),
            Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::ManaValueAtMost(1)]),
            Found::RevealToHand,
        ),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MAGE: &str = Card::TrinketMage;

    /// The artifacts of mana value 0 and 2 beside the prop's 1, for the scenarios about which
    /// mana values qualify.
    const CRYPT_AND_STAFF: &[&str] = &[Card::ManaCrypt, Card::StaffOfTheStoryteller];

    /// Mana Crypt (mana value 0) and Portable Hole (1) qualify; Staff of the Storyteller is an
    /// artifact of mana value 2, and Oust and the lands are not artifacts.
    const LIBRARY: &[&str] = &[
        Card::Island,
        Card::StaffOfTheStoryteller,
        Card::PortableHole,
        Card::Oust,
        Card::ManaCrypt,
        Card::Plains,
    ];

    /// The artifact prop (mana value 1) is the one trinket; Oust and the lands are not
    /// artifacts.
    const PROP_LIBRARY: &[&str] = &[Card::Island, Prop::Artifact, Card::Oust, Card::Plains];

    /// Casts the Mage and resolves it, then its trigger, up to the "you may".
    const ENTER: [Action; 3] = [ME.casts(MAGE), Resolve, Resolve];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Trinket Mage finds Mana Crypt")
            .interaction(CRYPT_AND_STAFF)
            .hand(P0, &[MAGE])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                // Finding nothing, the Crypt or the Hole.
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(Card::ManaCrypt),
                ME.has(Zone::Hand, Card::ManaCrypt, 1),
                ME.has_cards(Zone::Library, 5),
                ME.has(Zone::Battlefield, MAGE, 1),
            ]),
        Scenario::new("Trinket Mage finds Portable Hole")
            .hand(P0, &[MAGE])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, PROP_LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                // Finding nothing or the Hole.
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Card::PortableHole),
                ME.has(Zone::Hand, Card::PortableHole, 1),
                ME.has_cards(Zone::Library, 3),
            ]),
        Scenario::new("Trinket Mage finds nothing although Mana Crypt is there")
            .interaction(CRYPT_AND_STAFF)
            .hand(P0, &[MAGE])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                ME.done_picking(),
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Library, Card::ManaCrypt, 1),
                ME.has_cards(Zone::Library, 6),
            ]),
        // With no artifact of mana value 1 or less there is nothing to choose: the search finds
        // nothing without a question.
        Scenario::new("Trinket Mage searches a library without a trinket")
            .hand(P0, &[MAGE])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, &[Prop::Creature, Card::Oust, Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 3),
                stack(0),
            ]),
        Scenario::new("Trinket Mage declines to search")
            .hand(P0, &[MAGE])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, PROP_LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.no(),
                ME.has_cards(Zone::Hand, 0),
                // Unsearched and unshuffled.
                ME.library_top(PROP_LIBRARY),
            ]),
        // UI regression by-kac: a known library top must not add a separate Search? prompt.
        Scenario::new("Trinket Mage searches with a known library top")
            .hand(P0, &[MAGE])
            .battlefield(
                P0,
                &[Prop::Planeswalker, Card::Island, Card::Island, Card::Island],
            )
            .library(P0, PROP_LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(Prop::Planeswalker, 0), // the planeswalker's first ability
                ME.targets_player(ME),
                ME.no(),
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                ME.picks_up_to(Prop::Artifact),
                ME.has(Zone::Hand, Prop::Artifact, 1),
            ]),
    ];
}
