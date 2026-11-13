// Generated from the card store by `just card header`: notes go below the end marker.
//! Spellseeker {2}{U}
//! Creature — Human Wizard, 1/1
//!
//! When this creature enters, you may search your library for an instant or sorcery card with mana
//! value 2 or less, reveal it, put it into your hand, then shuffle.
// End of the generated card header.

use crate::prelude::*;

/// "An instant or sorcery card with mana value 2 or less".
///
/// "You may" covers the whole instruction, the shuffle with it: declining searches nothing and
/// shuffles nothing. Searching, the player may find nothing ([CR 701.23b]) and shuffles all the
/// same ([CR 701.24a]): "To shuffle a library ..., randomize the cards within it so that no player
/// knows their order." The card found is revealed ([CR 701.20a]): "To reveal a card, show that card
/// to all players for a brief time." Its mana value is that of its mana cost ([CR 202.3]): "The
/// mana value of an object is a number equal to the total amount of mana in its mana cost,
/// regardless of color."
///
/// [CR 701.23b]: https://yawgatog.com/resources/magic-rules/#R70123b
/// [CR 701.24a]: https://yawgatog.com/resources/magic-rules/#R70124a
/// [CR 701.20a]: https://yawgatog.com/resources/magic-rules/#R70120a
/// [CR 202.3]: https://yawgatog.com/resources/magic-rules/#R2023
pub static CARD: CardDef = Printed::Spellseeker
    .abilities(&[Ability::triggered(&Trigger::ENTERS).effect(&may(
        you(),
        &search(
            you(),
            Filter::All(&[
                Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                Filter::ManaValueAtMost(2),
            ]),
            Found::RevealToHand,
        ),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SEEKER: &str = Card::Spellseeker;

    /// The instant prop (Counterspell, mana value 2) and the sorcery prop (Oust, mana value 1)
    /// qualify; the untargeted-spell prop is a sorcery of mana value 3, the artifact prop is no
    /// instant or sorcery and the lands are neither.
    const LIBRARY: &[&str] = &[
        Card::Island,
        Prop::UntargetedSpell,
        Prop::Instant,
        Prop::Sorcery,
        Prop::Artifact,
        Card::Plains,
    ];

    /// Casts the Seeker and resolves it, then its trigger, up to the "you may".
    const ENTER: [Action; 3] = [ME.casts(SEEKER), Resolve, Resolve];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Spellseeker finds an instant")
            .hand(P0, &[SEEKER])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                // Finding nothing, the instant or the sorcery.
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(Prop::Instant),
                ME.has(Zone::Hand, Prop::Instant, 1),
                ME.has_cards(Zone::Library, 5),
                ME.has(Zone::Battlefield, SEEKER, 1),
            ]),
        Scenario::new("Spellseeker finds a sorcery")
            .hand(P0, &[SEEKER])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.yes(),
                ME.picks_up_to(Prop::Sorcery),
                ME.has(Zone::Hand, Prop::Sorcery, 1),
                ME.has_cards(Zone::Library, 5),
            ]),
        Scenario::new("Spellseeker finds nothing although an instant is there")
            .hand(P0, &[SEEKER])
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
                ME.has(Zone::Library, Prop::Instant, 1),
                ME.has_cards(Zone::Library, 6),
            ]),
        // Nothing in the library is an instant or sorcery of mana value 2 or less: the search
        // finds nothing without a question.
        Scenario::new("Spellseeker searches a library without a cheap spell")
            .hand(P0, &[SEEKER])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, &[Prop::UntargetedSpell, Prop::Artifact, Card::Plains])
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
        Scenario::new("Spellseeker declines to search")
            .hand(P0, &[SEEKER])
            .battlefield(P0, &[Card::Island; 3])
            .library(P0, LIBRARY)
            .library(P1, &[Card::Plains])
            .actions(&[
                ENTER[0],
                ENTER[1],
                ENTER[2],
                ME.no(),
                ME.has_cards(Zone::Hand, 0),
                // Unsearched and unshuffled.
                ME.library_top(LIBRARY),
            ]),
    ];
}
