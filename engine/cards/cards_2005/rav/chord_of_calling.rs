// Generated from the card store by `just card header`: notes go below the end marker.
//! Chord of Calling {X}{G}{G}{G}
//! Instant
//!
//! Convoke (Your creatures can help cast this spell. Each creature you tap while casting this spell
//! pays for {1} or one mana of that creature's color.)
//! Search your library for a creature card with mana value X or less, put it onto the battlefield,
//! then shuffle.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ChordOfCalling
    .keywords(Keywords::CONVOKE)
    .abilities(&[Ability::spell().effect(&search(
        you(),
        Filter::All(&[Filter::Types(Types::CREATURE), Filter::ManaValueAtMostX]),
        Found::OntoBattlefield,
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CHORD: &str = Card::ChordOfCalling;

    pub const SCENARIOS: &[Scenario] = &[
        // X is 1. Gold Myr could tap for {W} to pay the generic mana or tap for convoke, and
        // either pays it, so the player is asked; tapped for convoke, it makes no mana. The
        // three Forests pay {G}{G}{G}. Two Ornithopters qualify and one is found.
        Scenario::new("a mana creature convokes rather than tapping for mana")
            .interaction(&[Card::GoldMyr])
            .battlefield(
                P0,
                &[Card::Forest, Card::Forest, Card::Forest, Card::GoldMyr],
            )
            .hand(P0, &[CHORD])
            .library(
                P0,
                &[
                    Prop::ColorlessCreature,
                    Prop::GreenCreature,
                    Prop::ColorlessCreature,
                ],
            )
            .actions(&[
                ME.casts(CHORD),
                ME.x(1),
                ME.deciding(DecisionKind::ManaPayment, 2),
                ME.chooses(DecisionKind::ManaPayment, Card::GoldMyr),
                ME.tapped(Card::GoldMyr, 1),
                ME.tapped(Card::Forest, 3),
                ME.mana(ManaColor::White, 0),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Prop::ColorlessCreature),
                ME.controls(Prop::ColorlessCreature, 1),
                ME.has(Zone::Library, Prop::ColorlessCreature, 1),
                ME.has(Zone::Library, Prop::GreenCreature, 1),
            ]),
    ];
}
