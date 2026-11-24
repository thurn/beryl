// Generated from the card store by `just card header`: notes go below the end marker.
//! Burning Wish {1}{R}
//! Sorcery
//!
//! You may reveal a sorcery card you own from outside the game and put it into your hand. Exile
//! Burning Wish.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BurningWish
    .abilities(&[Ability::spell().effect(&sequence(&[
        wish(Filter::Types(Types::SORCERY)),
        exile(this()),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WISH: &str = Card::BurningWish;
    const BASE: Scenario = Scenario::setup()
        .hand(P0, &[WISH])
        .battlefield(P0, &[Card::Mountain, Card::Mountain, Card::Plains])
        .sideboard(P0, &[Prop::Sorcery, Prop::Instant]);

    pub const SCENARIOS: &[Scenario] = &[
        BASE.titled("bring in a sorcery and cast it")
            .battlefield(P1, &[Prop::Vanilla, Prop::Flier])
            .actions(&[
                ME.casts(WISH),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Prop::Sorcery),
                ME.has(Zone::Hand, Prop::Sorcery, 1),
                ME.has(Zone::Exile, WISH, 1),
                ME.casts(Prop::Sorcery),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                THEM.controls(Prop::Vanilla, 0),
            ]),
        BASE.titled("decline to bring in a card").actions(&[
            ME.casts(WISH),
            Resolve,
            ME.done_picking(),
            ME.has(Zone::Hand, Prop::Sorcery, 0),
            ME.has(Zone::Exile, WISH, 1),
        ]),
        BASE.titled("no eligible card outside the game")
            .sideboard(P0, &[Prop::Instant])
            .sideboard(P1, &[Prop::Sorcery])
            .library(P0, &[Prop::Sorcery])
            .graveyard(P0, &[Prop::Sorcery])
            .exile(P0, &[Prop::Sorcery])
            .actions(&[
                ME.casts(WISH),
                Resolve,
                ME.has(Zone::Hand, Prop::Sorcery, 0),
                ME.has(Zone::Library, Prop::Sorcery, 1),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                ME.has(Zone::Exile, Prop::Sorcery, 1),
                ME.has(Zone::Exile, WISH, 1),
            ]),
        BASE.titled("discard the sorcery brought into the game")
            .interaction(&[Card::ThirstForDiscovery])
            .hand(P0, &[WISH, Card::ThirstForDiscovery])
            .battlefield(
                P0,
                &[
                    Card::Mountain,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(
                P0,
                &[Card::Plains, Prop::Artifact, Card::Island, Card::Island],
            )
            .actions(&[
                ME.casts(WISH),
                Resolve,
                ME.picks_up_to(Prop::Sorcery),
                ME.casts(Card::ThirstForDiscovery),
                Resolve,
                ME.no(),
                ME.picks(Prop::Sorcery),
                ME.picks(Card::Plains),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
            ]),
        BASE.titled("two copies are one choice until both have been taken")
            .hand(P0, &[WISH, WISH, WISH])
            .sideboard(P0, &[Prop::Sorcery, Prop::Sorcery, WISH])
            .battlefield(P0, &[Card::Mountain; 6])
            .actions(&[
                ME.casts(WISH),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(Prop::Sorcery),
                ME.casts(WISH),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(Prop::Sorcery),
                ME.casts(WISH),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(WISH),
                ME.has(Zone::Hand, Prop::Sorcery, 2),
                ME.has(Zone::Hand, WISH, 1),
                ME.has(Zone::Exile, WISH, 3),
            ]),
    ];
}
