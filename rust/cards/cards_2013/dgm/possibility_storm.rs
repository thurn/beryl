// Generated from the card store by `just card header`: notes go below the end marker.
//! Possibility Storm {3}{R}{R}
//! Enchantment
//!
//! Whenever a player casts a spell from their hand, that player exiles it, then exiles cards from
//! the top of their library until they exile a card that shares a card type with it. That player
//! may cast that card without paying its mana cost. Then they put all cards exiled with this
//! enchantment on the bottom of their library in a random order.
// End of the generated card header.

use crate::prelude::*;

static FROM_HAND: Trigger = Trigger::casts_from(each_player(), Filter::Any, Zone::Hand);

pub static CARD: CardDef = Printed::PossibilityStorm
    .abilities(&[Ability::triggered(&FROM_HAND).effect(&sequence(&[
        Effect::ExileUntil {
            player: that_player(),
            matching: ExileMatch::SharesTypeWithTriggeringSpell,
            when_found: &may_cast_free(that_player(), those()),
        },
        put_into_library(those(), LibraryPosition::BottomRandom),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const STORM: &str = Card::PossibilityStorm;
    const POOL: &str = Card::KnowledgePool;
    const BOLT: &str = Card::LightningBolt;
    const REMOVAL: &str = Card::GetLost;
    const GEYSER: &str = Card::Braingeyser;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Shares one card type and the cast is from exile")
            .interaction(&[Card::Ornithopter])
            .battlefield(P0, &[STORM, Card::Plains])
            .hand(P0, &[Prop::Artifact])
            .library(P0, &[Card::Island, Card::Ornithopter, Card::Mountain])
            .actions(&[
                ME.casts(Prop::Artifact),
                stack(2),
                Resolve,
                ME.has(Zone::Exile, Prop::Artifact, 1),
                ME.has(Zone::Exile, Card::Ornithopter, 1),
                ME.chooses(DecisionKind::CastOffer, Card::Ornithopter),
                stack(1),
                ME.library_top(&[Card::Mountain]),
                ME.has_cards(Zone::Exile, 0),
                Resolve,
                ME.controls(Card::Ornithopter, 1),
            ]),
        Scenario::new("Declining returns the original and matching card to the library")
            .interaction(&[BOLT, REMOVAL])
            .battlefield(P0, &[STORM, Card::Mountain])
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[BOLT])
            .library(P0, &[Card::Island, REMOVAL, Card::Forest])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.answers(DecisionKind::CastOffer, Pick::Pass),
                ME.has_cards(Zone::Exile, 0),
                ME.library_top(&[Card::Forest]),
                ME.has(Zone::Library, BOLT, 1),
                ME.has(Zone::Library, REMOVAL, 1),
                stack(0),
            ]),
        Scenario::new("The Storm still finds a match after the Pool exiles the spell")
            .interaction(&[POOL, BOLT, REMOVAL, Card::ExaltedAngel])
            .battlefield(P0, &[STORM, Card::Mountain])
            .permanents(
                P1,
                &[
                    Permanent::new(POOL).exiled_with(&[Card::ExaltedAngel]),
                    Permanent::new(Prop::Flier),
                ],
            )
            .hand(P0, &[BOLT])
            .library(P0, &[REMOVAL, Card::Forest])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::ExaltedAngel),
                Resolve,
                Resolve,
                ME.chooses(DecisionKind::CastOffer, REMOVAL),
                ME.targets(Prop::Flier),
                Resolve,
                ME.has(Zone::Exile, BOLT, 1),
                ME.controls(Card::ExaltedAngel, 1),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
                stack(0),
            ]),
        Scenario::new("The Storm casts a matching sorcery with X equal to zero")
            .interaction(&[GEYSER])
            .battlefield(P0, &[STORM, Card::Plains])
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[Prop::Sorcery])
            .library(P0, &[GEYSER, Card::Forest])
            .actions(&[
                ME.casts(Prop::Sorcery),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, GEYSER),
                ME.targets_player(THEM),
                Resolve,
                THEM.has_cards(Zone::Hand, 0),
                ME.library_top(&[Card::Forest, Prop::Sorcery]),
            ]),
        Scenario::new("The Storm bottoms the original spell when no card matches")
            .interaction(&[BOLT])
            .battlefield(P0, &[STORM, Card::Mountain])
            .hand(P0, &[BOLT])
            .library(P0, &[Card::Forest, Card::Plains])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.has_cards(Zone::Library, 3),
                ME.has(Zone::Library, BOLT, 1),
                ME.has_cards(Zone::Exile, 0),
                THEM.life(20),
                stack(0),
            ]),
    ];
}
