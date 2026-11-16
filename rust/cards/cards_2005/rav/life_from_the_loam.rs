// Generated from the card store by `just card header`: notes go below the end marker.
//! Life from the Loam {1}{G}
//! Sorcery
//!
//! Return up to three target land cards from your graveyard to your hand.
//! Dredge 3 (If you would draw a card, you may mill three cards instead. If you do, return this
//! card from your graveyard to your hand.)
// End of the generated card header.

use crate::prelude::*;

/// "Return up to three target land cards from your graveyard to your hand"
/// ([CR 115.6]): "A spell or ability that requires targets may allow zero targets to be
/// chosen."
///
/// Dredge 3 ([CR 702.52a]) works only from its owner's graveyard, and only while their library
/// has at least three cards ([CR 702.52b]): "A player with fewer cards in their library than
/// the number required by a dredge ability can't mill any of them this way." Each draw is
/// offered it on its own, so of a draw of several cards one may be dredged and the rest drawn.
///
/// [CR 115.6]: https://yawgatog.com/resources/magic-rules/#R1156
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
/// [CR 702.52b]: https://yawgatog.com/resources/magic-rules/#R70252b
pub static CARD: CardDef = Printed::LifeFromTheLoam
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::up_to(
                3,
                TargetKind::GraveyardCard,
                LANDS_YOU_CONTROL,
            ))
            .effect(&return_to_hand(target())),
        Ability::replacing(&Replacement::dredge(3)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LOAM: &str = Card::LifeFromTheLoam;

    /// Lands to cast Life from the Loam with.
    const MANA: &[&str] = &[Card::Forest, Card::Forest];

    /// Four cards, so a dredge 3 leaves one to draw.
    const LIBRARY: &[&str] = &[Card::Island, Card::Plains, Card::Counterspell, Card::Island];

    /// The lands to flash in `Prop::FlashCreature` with: something to pass in the upkeep, so
    /// the draw step begins on a pass of P0's.
    const FLASH_MANA: &[&str] = &[Card::Swamp, Card::Island, Card::Island];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Life from the Loam returns three lands from its caster's graveyard")
            .battlefield(P0, MANA)
            .hand(P0, &[LOAM])
            .graveyard(
                P0,
                &[
                    Card::Plains,
                    Card::Island,
                    Card::Forest,
                    Prop::NonbasicLand,
                    Card::Counterspell,
                ],
            )
            .actions(&[
                ME.casts(LOAM),
                ME.target_count(3),
                ME.deciding(DecisionKind::Target, 4),
                ME.targets(Card::Plains),
                ME.targets(Card::Island),
                ME.targets(Card::Forest),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                ME.has(Zone::Graveyard, Prop::NonbasicLand, 1),
                ME.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.has(Zone::Graveyard, LOAM, 1),
            ]),
        // Up to three: with none chosen it still resolves, and does nothing.
        Scenario::new("Life from the Loam with no targets")
            .battlefield(P0, MANA)
            .hand(P0, &[LOAM])
            .graveyard(P0, &[Card::Plains])
            .actions(&[
                ME.casts(LOAM),
                ME.target_count(0),
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, Card::Plains, 1),
            ]),
        // A land in the opponent's graveyard is not a land card from P0's.
        Scenario::new("Life from the Loam can't target the opponent's lands")
            .battlefield(P0, MANA)
            .hand(P0, &[LOAM])
            .graveyard(P0, &[Card::Plains, Card::Forest])
            .graveyard(P1, &[Card::Island])
            .actions(&[
                ME.casts(LOAM),
                ME.target_count(1),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Card::Plains),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has(Zone::Graveyard, Card::Island, 1),
            ]),
        // The choice comes as the draw step begins, in no resolution: the dredge mills three
        // and Life from the Loam returns to the hand in place of the draw.
        Scenario::new("dredge in the draw step")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Hand, 2),
                ME.has_cards(Zone::Graveyard, 3),
                ME.library_top(&[Card::Island]),
            ]),
        // Declining, the player draws as they would have.
        Scenario::new("dredge declined")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.done_picking(),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has(Zone::Graveyard, LOAM, 1),
                ME.has_cards(Zone::Graveyard, 1),
            ]),
        // Each dredge card is a choice of its own; the one not chosen stays.
        Scenario::new("two dredge cards in the graveyard")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM, LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has(Zone::Graveyard, LOAM, 1),
                ME.has_cards(Zone::Graveyard, 4),
            ]),
        // With two cards in the library dredge 3 can't be used: no choice, a plain draw.
        Scenario::new("dredge with too few cards in the library")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.passes(),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has(Zone::Graveyard, LOAM, 1),
            ]),
        // Dredge replaces any draw, not only the draw step's. Of the 0 ability's three draws
        // the first is dredged, milling a second Life from the Loam, which replaces the next;
        // the third, with one card left in the library, is drawn. Then the ability asks its
        // own question, the two cards to put back.
        Scenario::new("dredge each draw of an ability on its own")
            .battlefield(P0, &[Prop::Planeswalker])
            .graveyard(P0, &[LOAM])
            .library(
                P0,
                &[
                    Card::Plains,
                    Card::Island,
                    LOAM,
                    Card::Counterspell,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(Prop::Planeswalker, 1), // 0: draw three, put two back
                Resolve,
                ME.picks_up_to(LOAM),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(LOAM),
                // The two Life from the Loam left are alike, so the second is put back silently.
                ME.picks(Card::Island),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Hand, 1),
                ME.has_cards(Zone::Library, 2),
                ME.has(Zone::Graveyard, Card::Plains, 2),
            ]),
        // Dredge works from the graveyard as the draw comes: Jace, Wielder of Mysteries' +1
        // mills Life from the Loam, and the draw that follows in the same resolution may be
        // dredged.
        Scenario::new("dredge a draw after the same ability milled it")
            .interaction(&[Card::JaceWielderOfMysteries])
            .battlefield(P0, &[Card::JaceWielderOfMysteries])
            .library(
                P0,
                &[
                    LOAM,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Counterspell,
                    Card::Plains,
                ],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(Card::JaceWielderOfMysteries, 1), // +1
                ME.targets_player(ME),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Hand, 1),
                ME.has_cards(Zone::Graveyard, 4),
                ME.library_top(&[Card::Plains]),
            ]),
        // A library of exactly three cards is enough (CR 702.52b): the dredge empties it and
        // the player doesn't lose for the draw that was replaced (CR 614.6).
        Scenario::new("dredge with exactly three cards in the library")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM])
            .library(P0, &[Card::Plains, Card::Island, Card::Island])
            .actions(&[
                ME.passes(),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Library, 0),
                ME.has_cards(Zone::Graveyard, 3),
            ]),
        // Dredge works only from its owner's graveyard (CR 702.52a): a Life from the Loam in
        // the opponent's graveyard offers P0 nothing, and P0 draws.
        Scenario::new("dredge in the opponent's graveyard isn't offered")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P1, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.has(Zone::Graveyard, LOAM, 1),
            ]),
        // Under Rest in Peace the three milled cards are exiled instead of reaching the
        // graveyard (CR 614.1a), but the dredge still happened, so Life from the Loam returns
        // (CR 702.52a).
        Scenario::new("dredge under Rest in Peace exiles the milled cards")
            .interaction(&[Card::RestInPeace])
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, FLASH_MANA)
            .battlefield(P1, &[Card::RestInPeace])
            .hand(P0, &[Prop::FlashCreature])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Graveyard, 0),
                ME.has_cards(Zone::Exile, 3),
            ]),
    ];
}
