// Generated from the card store by `just card header`: notes go below the end marker.
//! Tendrils of Agony {2}{B}{B}
//! Sorcery
//!
//! Target player loses 2 life and you gain 2 life.
//! Storm (When you cast this spell, copy it for each spell cast before it this turn. You may choose
//! new targets for the copies.)
// End of the generated card header.

use crate::prelude::*;

/// "Target player loses 2 life and you gain 2 life." Storm copies the spell once for each other
/// spell cast before it this turn ([CR 702.40a]), and each copy may have a new target.
///
/// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
pub static CARD: CardDef = Printed::TendrilsOfAgony
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::player())
            .effect(&sequence(&[
                lose_life(target_player(), 2),
                gain_life(you(), 2),
            ])),
        Ability::storm(),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TENDRILS: &str = Card::TendrilsOfAgony;
    const SCIMITAR: &str = Prop::Equipment;
    const SWAMP: &str = Card::Swamp;
    const SWAMPS: &[&str] = &[SWAMP; 4];

    pub const SCENARIOS: &[Scenario] = &[
        // Two spells before it: storm makes two copies, each aimed on its own.
        Scenario::new("Tendrils of Agony with storm 2")
            .battlefield(
                P0,
                &[SWAMP, SWAMP, SWAMP, SWAMP, Card::Plains, Card::Plains],
            )
            .hand(P0, &[SCIMITAR, SCIMITAR, TENDRILS])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                stack(2),
                Resolve,
                ME.targets_player(THEM),
                ME.targets_player(ME),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.life(16),
                ME.life(24),
                ME.has(Zone::Graveyard, TENDRILS, 1),
            ]),
        // The first spell of the turn: storm triggers and copies nothing.
        Scenario::new("Tendrils of Agony with no spell before it")
            .battlefield(P0, SWAMPS)
            .hand(P0, &[TENDRILS])
            .actions(&[
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                stack(2),
                Resolve,
                stack(1),
                Resolve,
                THEM.life(18),
                ME.life(22),
            ]),
        // A spell the opponent cast this turn counts for storm.
        Scenario::new("Tendrils of Agony counts the opponent's spell")
            .interaction(&[Card::LightningBolt])
            .battlefield(P0, SWAMPS)
            .hand(P0, &[TENDRILS])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[Card::LightningBolt])
            .actions(&[
                ME.passes(),
                THEM.casts(Card::LightningBolt),
                THEM.targets_player(ME),
                ME.life(17),
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                THEM.life(16),
                ME.life(21),
            ]),
        // Countered under its storm trigger, Tendrils of Agony is still copied as it last was
        // on the stack, and the copy may aim at another player.
        Scenario::new("Tendrils of Agony countered under its storm trigger is still copied")
            .battlefield(P0, &[SWAMP, SWAMP, SWAMP, SWAMP, Card::Plains])
            .hand(P0, &[SCIMITAR, TENDRILS])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                stack(2),
                THEM.casts(Card::Counterspell),
                ME.has(Zone::Graveyard, TENDRILS, 1),
                stack(1),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(18),
                ME.life(22),
            ]),
    ];
}
