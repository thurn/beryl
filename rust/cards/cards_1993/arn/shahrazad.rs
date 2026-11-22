// Generated from the card store by `just card header`: notes go below the end marker.
//! Shahrazad {W}{W}
//! Sorcery
//!
//! Players play a Magic subgame, using their libraries as their decks. Each player who doesn't win
//! the subgame loses half their life, rounded up.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Shahrazad
    .abilities(&[Ability::spell().effect(&sequence(&[
        Effect::Subgame,
        for_each_player(
            Players::SubgameNonwinners,
            &Effect::LoseLife {
                player: that_player(),
                amount: Amount::Half {
                    of: &Amount::Life(that_player()),
                    round_up: true,
                },
            },
        ),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPELL: &str = Card::Shahrazad;
    const BOLT: &str = Card::LightningBolt;
    const CRYPT: &str = Card::TormodsCrypt;
    const KEEP_ME: Action = Action::answer(P0, DecisionKind::KeepOrMulligan, Pick::Keep);
    const KEEP_THEM: Action = Action::answer(P1, DecisionKind::KeepOrMulligan, Pick::Keep);
    const BASE: Scenario = Scenario::setup()
        .hand(P0, &[SPELL])
        .battlefield(P0, &[Card::Plains; 2]);

    pub const SCENARIOS: &[Scenario] = &[
        BASE.titled("A player with fewer than seven library cards loses the subgame")
            .library(P0, &[Card::Plains; 8])
            .library(P1, &[Card::Mountain; 3])
            .life(P0, 17)
            .life(P1, 513)
            .actions(&[
                ME.casts(SPELL),
                Resolve,
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 1),
                THEM.has_cards(Zone::Hand, 3),
                THEM.has_cards(Zone::Library, 0),
                KEEP_THEM,
                KEEP_ME,
                ME.life(17),
                THEM.life(256),
                ME.has(Zone::Graveyard, SPELL, 1),
                THEM.has(Zone::Library, Card::Mountain, 3),
            ]),
        BASE.titled("Both players lose half their life when the subgame is a draw")
            .seed(1)
            .library(P0, &[Card::Plains; 6])
            .library(P1, &[Card::Mountain; 5])
            .life(P0, 21)
            .life(P1, 15)
            .actions(&[
                ME.casts(SPELL),
                Resolve,
                KEEP_ME,
                KEEP_THEM,
                ME.life(10),
                THEM.life(7),
                ME.has(Zone::Graveyard, SPELL, 1),
            ]),
        BASE.titled("Only libraries enter the subgame and even exiled cards return to them")
            .seed(1)
            .interaction(&[Card::Bloodghast, Card::Humility, BOLT, CRYPT])
            .hand(P0, &[SPELL, Card::Mountain])
            .battlefield(P0, &[Card::Plains, Card::Plains, Card::Humility])
            .graveyard(P0, &[Card::Bloodghast])
            .exile(P0, &[Card::Swamp])
            .library(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Prop::Vanilla,
                    Prop::Vanilla,
                ],
            )
            .library(
                P1,
                &[
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    BOLT,
                    BOLT,
                    CRYPT,
                    CRYPT,
                ],
            )
            .actions(&[
                ME.casts(SPELL),
                Resolve,
                ME.has_cards(Zone::Hand, 7),
                ME.has(Zone::Hand, Card::Mountain, 0),
                ME.has_cards(Zone::Battlefield, 0),
                ME.has_cards(Zone::Graveyard, 0),
                ME.has_cards(Zone::Exile, 0),
                KEEP_ME,
                KEEP_THEM,
                ME.plays(Card::Island),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.plays(Card::Mountain),
                advance_to(3, TurnStep::PrecombatMain),
                ME.plays(Card::Island),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.pt(Prop::Vanilla, 2, 1),
                advance_to(4, TurnStep::PrecombatMain),
                THEM.casts(BOLT),
                THEM.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                THEM.casts(CRYPT),
                Resolve,
                THEM.activates(CRYPT, 0),
                THEM.targets_player(ME),
                Resolve,
                ME.has(Zone::Exile, Prop::Vanilla, 1),
                THEM.has(Zone::Graveyard, CRYPT, 1),
                Action::Concede { player: P1 },
                ME.life(20),
                THEM.life(10),
                ME.has(Zone::Hand, Card::Mountain, 1),
                ME.has(Zone::Battlefield, Card::Humility, 1),
                ME.has(Zone::Graveyard, Card::Bloodghast, 1),
                ME.has(Zone::Exile, Card::Swamp, 1),
                ME.has(Zone::Library, Card::Island, 6),
                ME.has(Zone::Library, Prop::Vanilla, 2),
                THEM.has(Zone::Library, Card::Mountain, 6),
                THEM.has(Zone::Library, BOLT, 2),
                THEM.has(Zone::Library, CRYPT, 2),
            ]),
        BASE.titled("A nested subgame concession affects only its own parent game")
            .seed(1)
            .library(
                P0,
                &[
                    SPELL,
                    SPELL,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .library(P1, &[Card::Mountain; 9])
            .life(P0, 17)
            .life(P1, 23)
            .actions(&[
                ME.casts(SPELL),
                Resolve,
                KEEP_ME,
                KEEP_THEM,
                ME.plays(Card::Plains),
                advance_to(3, TurnStep::PrecombatMain),
                ME.plays(Card::Plains),
                ME.casts(SPELL),
                Resolve,
                ME.has_cards(Zone::Battlefield, 0),
                Action::Concede { player: P1 },
                ME.has(Zone::Graveyard, SPELL, 1),
                ME.life(20),
                THEM.life(10),
                Action::Concede { player: P0 },
                ME.life(8),
                THEM.life(23),
                ME.has(Zone::Graveyard, SPELL, 1),
                ME.has(Zone::Library, SPELL, 2),
            ]),
    ];
}
