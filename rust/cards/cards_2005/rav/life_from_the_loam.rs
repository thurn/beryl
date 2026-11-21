// Generated from the card store by `just card header`: notes go below the end marker.
//! Life from the Loam {1}{G}
//! Sorcery
//!
//! Return up to three target land cards from your graveyard to your hand.
//! Dredge 3 (If you would draw a card, you may mill three cards instead. If you do, return this
//! card from your graveyard to your hand.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::LifeFromTheLoam
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::up_to(
                3,
                TargetKind::GraveyardCard,
                Filter::All(&[Filter::Types(Types::LAND), Filter::Whose(&Players::You)]),
            ))
            .effect(&return_to_hand(target())),
        Ability::replacing(&Replacement::dredge(3)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LOAM: &str = Card::LifeFromTheLoam;

    /// Four cards, so a dredge 3 leaves one to draw.
    const LIBRARY: &[&str] = &[Card::Island, Card::Plains, Card::Counterspell, Card::Island];

    /// The lands to flash in `Prop::FlashCreature` with: something to pass in the upkeep, so
    /// the draw step begins on a pass of P0's.
    const FLASH_MANA: &[&str] = &[Card::Swamp, Card::Island, Card::Island];

    pub const SCENARIOS: &[Scenario] = &[
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
                // Milling and returning Loam are both done before priority resumes.
                stack(0),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Hand, 2),
                ME.has_cards(Zone::Graveyard, 3),
                ME.library_top(&[Card::Island]),
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
        // A search can put a card into hand while dredge is available without replacing the
        // move or offering a dredge choice.
        Scenario::new("putting a searched card into hand does not offer dredge")
            .interaction(&[Card::Spellseeker])
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::Spellseeker])
            .graveyard(P0, &[LOAM])
            .library(
                P0,
                &[Card::Island, Prop::Instant, Prop::Sorcery, Card::Plains],
            )
            .actions(&[
                ME.casts(Card::Spellseeker),
                Resolve,
                Resolve,
                ME.yes(),
                ME.picks_up_to(Prop::Instant),
                ME.has(Zone::Hand, Prop::Instant, 1),
                ME.has(Zone::Graveyard, LOAM, 1),
                ME.has_cards(Zone::Library, 3),
            ]),
    ];
}
