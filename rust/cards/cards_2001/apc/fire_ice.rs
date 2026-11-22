// Generated from the card store by `just card header`: notes go below the end marker.
//! Fire {1}{R}
//! Instant
//!
//! Fire deals 2 damage divided as you choose among one or two targets.
//!
//! Ice {1}{U}
//! Instant
//!
//! Tap target permanent.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::FireIce,
    PrintedShape::Fire,
    &[
        Printed::Fire.abilities(&[Ability::spell()
            .target(&TargetSpec::any_target().range(1, 2))
            .effect(&deal_two_divided())]),
        Printed::Ice.abilities(&[Ability::spell()
            .target(&TargetSpec::permanent(Filter::Any))
            .effect(&sequence(&[Effect::Tap(target()), draw(you(), 1)]))]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BOTH: &str = Card::FireIce;
    const FLIER: &str = Prop::Flier;

    const fn fire(name: &'static str) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[Card::Mountain, Card::Island])
            .battlefield(P1, &[FLIER])
            .hand(P0, &[BOTH])
    }

    pub const SCENARIOS: &[Scenario] = &[
        fire("Fire deals two damage to one target").actions(&[
            ME.casts(BOTH),
            ME.target_count(1),
            ME.targets_player(THEM),
            Resolve,
            THEM.life(18),
            ME.has(Zone::Graveyard, BOTH, 1),
            Action::assert(Assertion::Colors {
                player: P0,
                zone: Zone::Graveyard,
                card: BOTH,
                colors: Colors::RED.union(Colors::BLUE),
            }),
        ]),
        fire("Fire divides damage between a creature and a player").actions(&[
            ME.casts(BOTH),
            ME.target_count(2),
            ME.targets(FLIER),
            ME.targets_player(THEM),
            Resolve,
            THEM.life(19),
            THEM.has(Zone::Graveyard, FLIER, 1),
        ]),
        fire("Fire keeps the announced share when another target is gone")
            .interaction(&[Card::LightningBolt])
            .battlefield(P0, &[Card::Mountain; 3])
            .hand(P0, &[BOTH, Card::LightningBolt])
            .battlefield(P1, &[FLIER, Card::Mountain])
            .hand(P1, &[Card::LightningBolt])
            .actions(&[
                ME.casts(BOTH),
                ME.target_count(2),
                ME.targets(FLIER),
                ME.targets_player(THEM),
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets(FLIER),
                Resolve,
                Resolve,
                THEM.life(19),
            ]),
        fire("Spell Snare counters Fire as a spell with mana value two")
            .interaction(&[Card::SpellSnare])
            .battlefield(P1, &[Card::Island])
            .hand(P1, &[Card::SpellSnare])
            .actions(&[
                ME.casts(BOTH),
                ME.target_count(1),
                ME.targets_player(THEM),
                Action::assert(Assertion::Colors {
                    player: P0,
                    zone: Zone::Stack,
                    card: "Fire",
                    colors: Colors::RED,
                }),
                THEM.casts(Card::SpellSnare),
                THEM.life(20),
                ME.has(Zone::Graveyard, BOTH, 1),
            ]),
        fire("Ice taps a permanent and draws one card")
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts_face(BOTH, FaceIx::BACK),
                ME.targets(FLIER),
                Resolve,
                THEM.tapped(FLIER, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Hand, 1),
                ME.has(Zone::Graveyard, BOTH, 1),
            ]),
        fire("Ice can tap a land")
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                ME.casts_face(BOTH, FaceIx::BACK),
                ME.targets(Card::Plains),
                Resolve,
                THEM.tapped(Card::Plains, 1),
            ]),
        Scenario::new("Fire // Ice pays Force of Will's blue pitch cost")
            .interaction(&[Card::ForceOfWill])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[Card::ForceOfWill, BOTH])
            .hand(P1, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                Action::Cast {
                    player: P0,
                    card: Card::ForceOfWill,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.life(19),
                ME.has(Zone::Exile, BOTH, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        fire("Fork copies the Fire half and its division")
            .interaction(&[Card::Fork])
            .battlefield(P0, &[Card::Mountain; 4])
            .hand(P0, &[BOTH, Card::Fork])
            .actions(&[
                ME.casts(BOTH),
                ME.target_count(2),
                ME.targets_player(ME),
                ME.targets_player(THEM),
                ME.casts(Card::Fork),
                Resolve,
                ME.targets_player(ME),
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.life(18),
                THEM.life(18),
            ]),
        Scenario::new("Pithing Needle can name Ice")
            .interaction(&[Card::PithingNeedle])
            .hand(P0, &[Card::PithingNeedle, BOTH])
            .battlefield(P0, &[Card::Island])
            .actions(&[
                ME.casts(Card::PithingNeedle),
                Resolve,
                ME.names("Ice"),
                ME.controls(Card::PithingNeedle, 1),
            ]),
    ];
}
