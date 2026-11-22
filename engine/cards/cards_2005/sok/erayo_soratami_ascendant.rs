// Generated from the card store by `just card header`: notes go below the end marker.
//! Erayo, Soratami Ascendant {1}{U}
//! Legendary Creature — Moonfolk Monk, 1/1
//!
//! Flying
//! Whenever the fourth spell of a turn is cast, flip Erayo.
//!
//! Erayo's Essence
//! Legendary Enchantment
//!
//! Whenever an opponent casts their first spell each turn, counter that spell.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::ErayoSoratamiAscendant,
    PrintedShape::ErayoSoratamiAscendant,
    &[
        Printed::ErayoSoratamiAscendant
            .keywords(Keywords::FLYING)
            .abilities(&[Ability::triggered(
                &Trigger::casts(each_player(), Filter::Any).numbered(CastNumber::InTurn(4)),
            )
            .effect(&flip(this()))]),
        Printed::ErayosEssence.abilities(&[Ability::triggered(
            &Trigger::casts(Players::EachOpponent, Filter::Any).numbered(CastNumber::ByCaster(1)),
        )
        .effect(&counter(Objects::TriggeredOn))]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ERAYO: &str = Card::ErayoSoratamiAscendant;
    const ESSENCE: &str = "Erayo's Essence";
    const BOLT: &str = Card::LightningBolt;
    const BOUNCE: &str = Prop::Removal;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the fourth spell flips Erayo and Essence counters only a new first spell")
            .interaction(&[BOLT])
            .battlefield(
                P0,
                &[
                    ERAYO,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .battlefield(P1, &[Card::Mountain, Card::Mountain, Card::Mountain])
            .hand(P0, &[BOLT; 4])
            .hand(P1, &[BOLT, BOLT, BOLT, BOLT])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.controls(ERAYO, 1),
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                ME.controls(ESSENCE, 1),
                ME.creature(ESSENCE, false),
                // The spell that made Erayo flip is already on the stack. It still resolves.
                Resolve,
                ME.life(17),
                Resolve,
                THEM.life(11),
                // This is their second cast this turn.
                ME.passes(),
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                ME.life(14),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                // Their first spell is the second spell overall. Essence counts by caster.
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(8),
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                ME.life(14),
                // Their second spell on this turn resolves too.
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                ME.life(11),
            ]),
        Scenario::new("a flipped Erayo returned to hand can be recast unflipped")
            .interaction(&[BOLT])
            .permanents(
                P0,
                &[
                    Permanent::new(ERAYO).flipped(),
                    Permanent::new(Card::Island),
                    Permanent::new(Card::Island),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Prop::Artifact),
                ],
            )
            .battlefield(
                P1,
                &[Card::Island, Card::Island, Card::Island, Card::Mountain],
            )
            .hand(P1, &[BOLT, BOUNCE])
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                Resolve,
                ME.life(20),
                ME.passes(),
                THEM.casts(BOUNCE),
                THEM.targets(ESSENCE),
                ME.has(Zone::Hand, ERAYO, 1),
                ME.casts(ERAYO),
                ME.creature(ERAYO, true),
                ME.controls(ESSENCE, 0),
            ]),
        Scenario::new("two copies of Erayo's Essence invoke the legend rule")
            .interaction(&[BOLT])
            .permanents(
                P0,
                &[
                    Permanent::new(ERAYO)
                        .flipped()
                        .counters(CounterKind::PlusOne, 1),
                    Permanent::new(ERAYO),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Card::Mountain),
                    Permanent::new(Card::Mountain),
                ],
            )
            .hand(P0, &[BOLT; 4])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                ME.picks(ESSENCE),
                ME.controls(ESSENCE, 1),
                ME.has(Zone::Graveyard, ERAYO, 1),
            ]),
        Scenario::new("the legend rule applies separately to the two Erayo names")
            .permanents(
                P0,
                &[
                    Permanent::new(ERAYO).flipped(),
                    Permanent::new(ERAYO).sick(),
                    Permanent::new(Card::Island),
                    Permanent::new(Card::Island),
                ],
            )
            .hand(P0, &[ERAYO])
            .actions(&[
                ME.controls(ESSENCE, 1),
                ME.controls(ERAYO, 1),
                ME.casts(ERAYO),
                Resolve,
                ME.controls(ESSENCE, 1),
                ME.controls(ERAYO, 1),
                ME.has(Zone::Graveyard, ERAYO, 1),
            ]),
    ];
}
