// Generated from the card store by `just card header`: notes go below the end marker.
//! Abhorrent Oculus {2}{U}
//! Creature — Eye, 5/5
//!
//! As an additional cost to cast this spell, exile six cards from your graveyard.
//! Flying
//! At the beginning of each opponent's upkeep, manifest dread. (Look at the top two cards of your
//! library. Put one onto the battlefield face down as a 2/2 creature and the other into your
//! graveyard. Turn it face up any time for its mana cost if it's a creature card.)
// End of the generated card header.

use crate::prelude::*;

/// The six cards are an additional cost
/// ([CR 118.8](https://yawgatog.com/resources/magic-rules/#R1188)): "Some spells and abilities
/// have additional costs. An additional cost is a cost listed in a spell's rules text, or
/// applied to a spell or ability from another effect, that its controller must pay at the same
/// time they pay the spell's mana cost or the ability's activation cost." The trigger is the
/// opponent's upkeep's alone
/// ([CR 503.1a](https://yawgatog.com/resources/magic-rules/#R5031a)): "Any abilities that
/// triggered during the untap step and any abilities that triggered at the beginning of the
/// upkeep are put onto the stack before the active player gets priority".
///
/// Manifest dread ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "Look
/// at the top two cards of your library. Manifest one of them, then put the cards you looked
/// at that were not manifested this way into your graveyard." The ability's controller does,
/// whether or not Oculus is still there
/// ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)).
pub static CARD: CardDef = Printed::AbhorrentOculus
    .keywords(Keywords::FLYING)
    .abilities(&[
        Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::EachOpponent))
            .effect(&manifest_dread(you())),
    ])
    .additional_cost(&[CostPart::exile(6, Zone::Graveyard, Filter::Any)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const OCULUS: &str = Card::AbhorrentOculus;

    // Flash gives the controller a live stop on the opponent's turn, before the next draw.
    const OCULUS_WITH_FLASH_MANA: &[&str] = &[OCULUS, Card::Island, Card::Island];

    /// Six graveyard cards and a seventh at the bottom, top first.
    const SEVEN: &[&str] = &[
        Card::Counterspell,
        Prop::Removal,
        Prop::UntargetedSpell,
        Prop::Artifact,
        Card::Oust,
        Prop::Planeswalker,
        Card::Plains,
    ];

    /// From player 0's turn 1 to player 1's upkeep, where the manifest dread trigger waits.
    const TO_THEIR_UPKEEP: Action = advance_to(2, TurnStep::Upkeep);

    pub const SCENARIOS: &[Scenario] = &[
        // With five cards in the graveyard the additional cost can't be paid (CR 118.3), so
        // only passing and the land are offered.
        Scenario::new("Abhorrent Oculus can't be cast with five cards in the graveyard")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[OCULUS, Card::Island])
            .graveyard(P0, SEVEN.split_at(5).0)
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        // The cards are chosen from the bottom of the graveyard up, so the first pick says which
        // of the bottom two stays; the other five go unasked.
        Scenario::new("Abhorrent Oculus exiles six of seven graveyard cards as it's cast")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[OCULUS])
            .graveyard(P0, SEVEN)
            .actions(&[
                ME.casts(OCULUS),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Prop::Planeswalker),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has_cards(Zone::Exile, 6),
                ME.tapped(Card::Island, 3),
                Resolve,
                ME.pt(OCULUS, 5, 5),
            ]),
        // Snapcaster Mage is manifested and Counterspell, the other card looked at, goes to the
        // graveyard. Player 0's own upkeep triggers nothing.
        Scenario::new("Abhorrent Oculus manifests dread on each opponent's upkeep only")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[OCULUS])
            .library(
                P0,
                &[
                    Card::SnapcasterMage,
                    Card::Counterspell,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                stack(1),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::SnapcasterMage),
                ME.face_down(Card::SnapcasterMage, 1),
                // CR 708.2a: a 2/2 creature, whatever its card.
                ME.pt(Card::SnapcasterMage, 2, 2),
                ME.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.library_top(&[Card::Island, Card::Island]),
                advance_to(3, TurnStep::Upkeep),
                stack(0),
            ]),
        // CR 701.62a: with one card to look at, that one is manifested, unasked.
        Scenario::new("Manifest dread with one card in the library manifests it")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, OCULUS_WITH_FLASH_MANA)
            .hand(P0, &[Card::SnapcasterMage])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                Resolve,
                ME.face_down(Card::Island, 1),
                ME.has_cards(Zone::Library, 0),
            ]),
        // CR 701.62a: with no cards to look at, the trigger resolves without manifesting.
        Scenario::new("Manifest dread with an empty library does nothing")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, OCULUS_WITH_FLASH_MANA)
            .hand(P0, &[Card::SnapcasterMage])
            .library(P0, &[])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                stack(1),
                Resolve,
                ME.has_cards(Zone::Library, 0),
                ME.has_cards(Zone::Battlefield, 3),
                stack(0),
            ]),
        // CR 116.2b: player 0 turns it face up in player 1's upkeep, paying {1}{U}. Snapcaster
        // Mage's enters trigger, which Counterspell in the graveyard would give a target, doesn't
        // trigger (CR 708.8).
        Scenario::new("A manifested creature turns face up for its mana cost, entering no more")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[OCULUS, Card::Island, Card::Island])
            .graveyard(P0, &[Card::Counterspell])
            .library(P0, &[Card::SnapcasterMage, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                Resolve,
                ME.picks(Card::SnapcasterMage),
                Action::TurnFaceUp {
                    player: P0,
                    card: Card::SnapcasterMage,
                },
                ME.face_down(Card::SnapcasterMage, 0),
                ME.tapped(Card::Island, 2),
                ME.pt(Card::SnapcasterMage, 2, 1),
                stack(0),
            ]),
        // CR 701.40b: a face-down Counterspell is a 2/2 creature, but its card isn't a creature
        // card, so it can't be turned face up: player 0 may only pass or cast Snapcaster Mage.
        Scenario::new("A manifested noncreature card can't be turned face up")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[OCULUS, Card::Island, Card::Island])
            .hand(P0, &[Card::SnapcasterMage])
            .library(P0, &[Card::Counterspell, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                Resolve,
                ME.picks(Card::Counterspell),
                ME.creature(Card::Counterspell, true),
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        // CR 708.9: Oust puts the face-down Snapcaster Mage second from the top of its owner's
        // library, revealed as it goes. Its controller gains the 3 life.
        Scenario::new("A face-down permanent leaving the battlefield is revealed")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[OCULUS])
            .library(P0, &[Card::SnapcasterMage, Card::Island, Card::Island])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::Oust])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                Resolve,
                ME.picks(Card::SnapcasterMage),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::Oust),
                THEM.targets(Card::SnapcasterMage),
                Resolve,
                ME.library_top(&[Card::Island, Card::SnapcasterMage]),
                ME.life(23),
                // On to combat: the tests read what each player knows here.
                THEM.passes(),
            ]),
        // CR 708.2: the copy Fractured Identity makes of the face-down Snapcaster Mage is a 2/2
        // with no text, for player 1, the one player other than its controller.
        Scenario::new("Fractured Identity copies a face-down permanent as a 2/2")
            .interaction(&[Card::FracturedIdentity])
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[OCULUS])
            .library(P0, &[Card::SnapcasterMage, Card::Island, Card::Island])
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P1, &[Card::FracturedIdentity])
            .library(P1, &[Card::Plains])
            .actions(&[
                TO_THEIR_UPKEEP,
                Resolve,
                ME.picks(Card::SnapcasterMage),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::FracturedIdentity),
                THEM.targets(Card::SnapcasterMage),
                Resolve,
                ME.has(Zone::Exile, Card::SnapcasterMage, 1),
                THEM.has_cards(Zone::Battlefield, 6),
                // On to combat: the tests read the copy here.
                THEM.passes(),
            ]),
    ];
}
