// Generated from the card store by `just card header`: notes go below the end marker.
//! Knowledge Pool {6}
//! Artifact
//!
//! Imprint — When this artifact enters, each player exiles the top three cards of their library.
//! Whenever a player casts a spell from their hand, that player exiles it. If the player does, they
//! may cast a spell from among other cards exiled with this artifact without paying its mana cost.
// End of the generated card header.

use crate::prelude::*;

static FROM_HAND: Trigger = Trigger::casts_from(each_player(), Filter::Any, Zone::Hand);

pub static CARD: CardDef = Printed::KnowledgePool
    .abilities(&[
        when_enters(&exile_linked(top_of_library(&each_player(), 3))).imprint(),
        Ability::triggered(&FROM_HAND).effect(&sequence(&[
            exile_linked(Objects::TriggeredOnIn(Zone::Stack)),
            if_then(
                Condition::Did,
                &may_cast_free(that_player(), except_those(&exiled_with_this())),
            ),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const POOL: &str = Card::KnowledgePool;
    const STORM: &str = Card::PossibilityStorm;
    const BOLT: &str = Card::LightningBolt;
    const COUNTER: &str = Prop::Instant;
    const ELF: &str = Card::BloodbraidElf;
    const ANGEL: &str = Card::ExaltedAngel;
    const SEVEN_MOUNTAINS: &[&str] = &[Card::Mountain; 7];
    const MAGE: &str = Card::SnapcasterMage;
    const CAULDRON: &str = Card::AgathasSoulCauldron;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Imprint exiles every remaining card from short libraries")
            .battlefield(P0, &[Card::Mountain; 6])
            .hand(P0, &[POOL])
            .library(P0, &[Card::Mountain, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(POOL),
                Resolve,
                Resolve,
                ME.has(Zone::Exile, Card::Mountain, 1),
                ME.has(Zone::Exile, Card::Island, 1),
                THEM.has(Zone::Exile, Card::Plains, 1),
                ME.has_cards(Zone::Library, 0),
                THEM.has_cards(Zone::Library, 0),
            ]),
        Scenario::new("Imprint takes three cards and the caster can use an opponent's card")
            .interaction(&[BOLT, ANGEL])
            .battlefield(P0, SEVEN_MOUNTAINS)
            .hand(P0, &[POOL, BOLT])
            .library(
                P0,
                &[Card::Mountain, Card::Island, Card::Plains, Card::Forest],
            )
            .library(P1, &[ANGEL, Card::Island, Card::Plains, Card::Mountain])
            .actions(&[
                ME.casts(POOL),
                Resolve,
                Resolve,
                ME.has_cards(Zone::Exile, 3),
                THEM.has_cards(Zone::Exile, 3),
                ME.library_top(&[Card::Forest]),
                THEM.library_top(&[Card::Mountain]),
                ME.casts(BOLT),
                ME.targets_player(THEM),
                stack(2),
                Resolve,
                ME.deciding(DecisionKind::CastOffer, 2),
                ME.chooses(DecisionKind::CastOffer, ANGEL),
                stack(1),
                ME.has(Zone::Exile, BOLT, 1),
                Resolve,
                ME.controls(ANGEL, 1),
                stack(0),
                THEM.life(20),
            ]),
        Scenario::new("Only the first Pool to resolve offers its own cards")
            .interaction(&[BOLT, ANGEL])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[BOLT]),
                    Permanent::new(Card::Mountain),
                ],
            )
            .permanents(P1, &[Permanent::new(POOL).exiled_with(&[ANGEL])])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                ME.deciding(DecisionKind::CastOffer, 2),
                ME.chooses(DecisionKind::CastOffer, ANGEL),
                Resolve,
                Resolve,
                stack(0),
                ME.controls(ANGEL, 1),
                ME.has(Zone::Exile, BOLT, 2),
            ]),
        Scenario::new("An exiled spell can be cast on a later trigger")
            .interaction(&[BOLT])
            .battlefield(P0, &[POOL, Card::Mountain, Card::Island, Card::Island])
            .hand(P0, &[BOLT, Prop::Vanilla])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                stack(0),
                ME.has(Zone::Exile, BOLT, 1),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.deciding(DecisionKind::CastOffer, 2),
                ME.chooses(DecisionKind::CastOffer, BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
                ME.has(Zone::Exile, Prop::Vanilla, 1),
            ]),
        Scenario::new("A countered spell gives the Pool no cast to offer")
            .interaction(&[BOLT, ANGEL])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[COUNTER, ANGEL]),
                    Permanent::new(Card::Mountain),
                ],
            )
            .battlefield(P1, &[Card::Mountain])
            .hand(P0, &[BOLT])
            .hand(P1, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                THEM.chooses(DecisionKind::CastOffer, COUNTER),
                Resolve,
                ME.has(Zone::Graveyard, BOLT, 1),
                Resolve,
                stack(0),
                ME.has(Zone::Exile, ANGEL, 1),
                THEM.life(20),
            ]),
        Scenario::new("A spell cast with flashback bypasses the Pool")
            .interaction(&[BOLT, MAGE])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[MAGE]),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Card::Mountain),
                ],
            )
            .hand(P0, &[BOLT])
            .graveyard(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, MAGE),
                Resolve,
                Resolve,
                stack(0),
                ME.casts(BOLT),
                ME.targets_player(THEM),
                stack(1),
                Resolve,
                THEM.life(17),
                ME.has(Zone::Exile, BOLT, 2),
            ]),
        Scenario::new("A free cast still pays its mandatory additional cost")
            .interaction(&[BOLT, Card::VillageRites])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[Card::VillageRites]),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Prop::Vanilla),
                ],
            )
            .hand(P0, &[BOLT])
            .library(P0, &[Card::Island, Card::Forest])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::VillageRites),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                Resolve,
                ME.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("A free cast can still be kicked")
            .interaction(&[BOLT, Card::UrborgEmissary])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[Card::UrborgEmissary]),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Card::Island),
                    Permanent::new(Card::Island),
                ],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.answers(
                    DecisionKind::CastOffer,
                    Pick::Cast {
                        card: Card::UrborgEmissary,
                        face: FaceIx::FRONT,
                        costs: Castings::only(0),
                    },
                ),
                Resolve,
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.has(Zone::Hand, Prop::Vanilla, 1),
            ]),
        Scenario::new("The original spell's cascade trigger still resolves")
            .interaction(&[ELF, BOLT])
            .battlefield(
                P0,
                &[Card::Mountain, Card::Forest, Card::Forest, Card::Forest],
            )
            .battlefield(P1, &[POOL])
            .hand(P0, &[ELF])
            .library(P0, &[BOLT])
            .actions(&[
                ME.casts(ELF),
                stack(3),
                Resolve,
                Resolve,
                ME.chooses(DecisionKind::CastOffer, BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
                ME.has(Zone::Exile, ELF, 1),
            ]),
        Scenario::new("When the Storm resolves first the Pool has nothing to exile")
            .interaction(&[STORM, BOLT, ANGEL, Card::GetLost])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[ANGEL]),
                    Permanent::new(Card::Mountain),
                ],
            )
            .hand(P0, &[BOLT])
            .library(P0, &[Card::GetLost, Card::Forest])
            .battlefield(P1, &[STORM, Prop::Flier])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, Card::GetLost),
                ME.targets(Prop::Flier),
                Resolve,
                Resolve,
                stack(0),
                ME.has(Zone::Exile, ANGEL, 1),
                ME.library_top(&[Card::Forest, BOLT]),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // Agatha's Soul Cauldron's static ability reads the cards exiled with an object, so the
        // Pool's exile of the spell leaves the layer results stale before the offer reads
        // whether the player may cast the card now.
        Scenario::new("The offer follows an exile that the layers read")
            .interaction(&[CAULDRON, BOLT, ANGEL])
            .permanents(
                P0,
                &[
                    Permanent::new(POOL).exiled_with(&[ANGEL]),
                    Permanent::new(Card::Mountain),
                ],
            )
            .battlefield(P1, &[CAULDRON])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.chooses(DecisionKind::CastOffer, ANGEL),
                Resolve,
                stack(0),
                ME.controls(ANGEL, 1),
                ME.has(Zone::Exile, BOLT, 1),
                THEM.life(20),
            ]),
    ];
}
