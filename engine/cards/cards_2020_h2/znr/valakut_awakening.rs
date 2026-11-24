// Generated from the card store by `just card header`: notes go below the end marker.
//! Valakut Awakening {2}{R}
//! Instant
//!
//! Put any number of cards from your hand on the bottom of your library, then draw that many cards
//! plus one.
//!
//! Valakut Stoneforge
//! Land
//!
//! This land enters tapped.
//! {T}: Add {R}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::ValakutAwakening,
    PrintedShape::ValakutAwakening,
    &[
        Printed::ValakutAwakening.abilities(&[Ability::spell().effect(&sequence(&[
            put_any_on_bottom(you()),
            Effect::Draw {
                player: you(),
                count: Amount::Plus(1, &Amount::ThatMany),
            },
        ]))]),
        Printed::ValakutStoneforge.abilities(&[
            Ability::replacing(&Replacement::ENTERS_TAPPED),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Red)),
        ]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const AWAKENING: &str = Card::ValakutAwakening;
    const STONEFORGE: &str = Card::ValakutStoneforge;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "Valakut Awakening puts chosen cards on the bottom and draws that many plus one",
        )
        .hand(P0, &[AWAKENING, Card::Island, Card::Forest])
        .battlefield(P0, &[Card::Mountain; 3])
        .library(P0, &[Card::Mountain, Card::Swamp, Card::Plains])
        .actions(&[
            ME.casts(AWAKENING),
            Resolve,
            ME.picks_up_to(Card::Island),
            ME.done_picking(),
            ME.has(Zone::Hand, Card::Island, 0),
            ME.has(Zone::Hand, Card::Forest, 1),
            ME.has(Zone::Hand, Card::Mountain, 1),
            ME.has(Zone::Hand, Card::Swamp, 1),
            ME.has(Zone::Library, Card::Island, 1),
        ]),
        Scenario::new("Valakut Awakening puts nothing on the bottom and draws one card")
            .hand(P0, &[AWAKENING, Card::Island])
            .battlefield(P0, &[Card::Mountain; 3])
            .library(P0, &[Card::Mountain, Card::Swamp])
            .actions(&[
                ME.casts(AWAKENING),
                Resolve,
                ME.done_picking(),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has(Zone::Hand, Card::Mountain, 1),
                ME.has(Zone::Library, Card::Swamp, 1),
            ]),
        Scenario::new("Pithing Needle can name Valakut Stoneforge, the back face")
            .interaction(&[Card::PithingNeedle])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::PithingNeedle, Card::Island, AWAKENING])
            .actions(&[
                ME.casts(Card::PithingNeedle),
                Resolve,
                ME.names(STONEFORGE),
                ME.controls(Card::PithingNeedle, 1),
            ]),
        Scenario::new("Valakut Stoneforge enters tapped and uses the land play")
            .hand(P0, &[AWAKENING, Card::Island])
            .battlefield(P0, &[Prop::Flier])
            .actions(&[
                ME.plays(AWAKENING),
                ME.tapped(STONEFORGE, 1),
                Action::assert(Assertion::ManaValue {
                    object: SnapshotRef {
                        zone: Zone::Battlefield,
                        player: P0,
                        name: Some(STONEFORGE),
                        index: 0,
                    },
                    value: 0,
                }),
                ME.deciding(DecisionKind::Attacker, 2),
                ME.has(Zone::Hand, Card::Island, 1),
            ]),
        Scenario::new("Valakut Stoneforge makes red mana after untapping")
            .interaction(&[Card::LightningBolt])
            .hand(P0, &[AWAKENING, Card::LightningBolt])
            .actions(&[
                ME.plays(AWAKENING),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(Card::LightningBolt),
                ME.targets_player(THEM),
                Resolve,
                ME.tapped(STONEFORGE, 1),
                THEM.life(17),
            ]),
        Scenario::new("Sylvan Scrying can find a land but cannot find Valakut Awakening")
            .interaction(&[Card::SylvanScrying])
            .hand(P0, &[Card::SylvanScrying])
            .battlefield(P0, &[Card::Forest; 2])
            .library(P0, &[AWAKENING, Card::Mountain])
            .actions(&[
                ME.casts(Card::SylvanScrying),
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Card::Mountain),
                ME.has(Zone::Hand, Card::Mountain, 1),
                ME.has(Zone::Library, AWAKENING, 1),
            ]),
        Scenario::new("A destroyed Valakut Stoneforge is an instant in the graveyard")
            .interaction(&[Card::LightningBolt, Card::SnapcasterMage, Card::StripMine])
            .hand(P0, &[AWAKENING, Card::SnapcasterMage])
            .graveyard(P0, &[Card::LightningBolt])
            .battlefield(P0, &[Card::Island; 2])
            .battlefield(P1, &[Card::StripMine])
            .actions(&[
                ME.plays(AWAKENING),
                ME.passes(),
                THEM.activates(Card::StripMine, 1),
                THEM.targets(STONEFORGE),
                Resolve,
                ME.has(Zone::Graveyard, AWAKENING, 1),
                ME.casts(Card::SnapcasterMage),
                Resolve,
                ME.targets(AWAKENING),
                Resolve,
            ]),
        Scenario::new("Valakut Stoneforge returns to hand as Valakut Awakening")
            .interaction(&[Card::UrborgEmissary])
            .hand(P0, &[AWAKENING, Card::Island])
            .hand(P1, &[Card::UrborgEmissary])
            .battlefield(
                P1,
                &[
                    Card::Swamp,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.plays(AWAKENING),
                advance_to(2, TurnStep::PrecombatMain),
                Action::Cast {
                    player: P1,
                    card: Card::UrborgEmissary,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                THEM.targets(STONEFORGE),
                Resolve,
                ME.colored(Zone::Hand, AWAKENING, Colors::RED),
            ]),
        Scenario::new("Flickerwisp cannot return Valakut Awakening to the battlefield")
            .interaction(&[Card::Flickerwisp])
            .hand(P0, &[AWAKENING, Card::Flickerwisp])
            .battlefield(P0, &[Card::Plains; 3])
            .actions(&[
                ME.plays(AWAKENING),
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(STONEFORGE),
                Resolve,
                advance_to(1, TurnStep::End),
                Resolve,
                ME.has(Zone::Exile, AWAKENING, 1),
                ME.controls(STONEFORGE, 0),
            ]),
    ];
}
