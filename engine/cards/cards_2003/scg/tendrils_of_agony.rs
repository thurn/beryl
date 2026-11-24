// Generated from the card store by `just card header`: notes go below the end marker.
//! Tendrils of Agony {2}{B}{B}
//! Sorcery
//!
//! Target player loses 2 life and you gain 2 life.
//! Storm (When you cast this spell, copy it for each spell cast before it this turn. You may choose
//! new targets for the copies.)
// End of the generated card header.

use crate::prelude::*;

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
                THEM.casts(Card::Counterspell),
                ME.has(Zone::Graveyard, TENDRILS, 1),
                stack(1),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(18),
                ME.life(22),
            ]),
        // Only the two spells cast before the second Tendrils count for its storm trigger.
        Scenario::new("Tendrils of Agony does not count storm copies")
            .battlefield(P0, &[SWAMP; 9])
            .hand(P0, &[SCIMITAR, TENDRILS, TENDRILS])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.life(10),
                ME.life(30),
            ]),
        // A spell that counters the storm trigger leaves the original Tendrils alone.
        Scenario::new("countering Tendrils of Agony's storm trigger prevents every copy")
            .interaction(&[Card::Stifle])
            .battlefield(P0, &[SWAMP; 5])
            .hand(P0, &[SCIMITAR, TENDRILS])
            .battlefield(P1, &[Card::Island])
            .hand(P1, &[Card::Stifle])
            .actions(&[
                ME.casts(SCIMITAR),
                Resolve,
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                THEM.casts(Card::Stifle),
                Resolve,
                stack(0),
                THEM.life(18),
                ME.life(22),
            ]),
        // Storm counts the Diamond that was cast with escape and then countered, and the
        // spell that countered it.
        Scenario::new("Tendrils of Agony counts a countered spell cast from the graveyard")
            .interaction(&[Card::UnderworldBreach, Card::LionsEyeDiamond])
            .battlefield(P0, &[Card::UnderworldBreach, SWAMP, SWAMP, SWAMP, SWAMP])
            .graveyard(
                P0,
                &[
                    Card::LionsEyeDiamond,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[TENDRILS])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.escapes(Card::LionsEyeDiamond),
                THEM.casts(Prop::Instant),
                ME.has(Zone::Graveyard, Card::LionsEyeDiamond, 1),
                ME.casts(TENDRILS),
                ME.targets_player(THEM),
                Resolve,
                ME.targets_player(THEM),
                ME.targets_player(THEM),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.life(14),
                ME.life(26),
            ]),
    ];
}
