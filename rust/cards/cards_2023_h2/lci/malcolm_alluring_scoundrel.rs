// Generated from the card store by `just card header`: notes go below the end marker.
//! Malcolm, Alluring Scoundrel {1}{U}
//! Legendary Creature — Siren Pirate, 2/1
//!
//! Flash
//! Flying
//! Whenever Malcolm deals combat damage to a player, put a chorus counter on it. Draw a card, then
//! discard a card. If there are four or more chorus counters on Malcolm, you may cast the discarded
//! card without paying its mana cost.
// End of the generated card header.

use crate::prelude::*;

/// The counter, the loot, and the offer to cast the discarded card as the ability finishes
/// resolving ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). Without its
/// mana cost it is cast for an alternative cost
/// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)), so a land, which can't be
/// cast, is never offered.
///
/// The count is read from Malcolm on the battlefield. Had he left before the ability
/// resolved, his last-known chorus count would decide
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)); the engine keeps no
/// counters in last-known information, so the cast is then not offered.
pub static CARD: CardDef = Printed::MalcolmAlluringScoundrel
    .keywords(Keywords::FLASH.union(Keywords::FLYING))
    .abilities(&[Ability::triggered(&Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER).effect(
        &sequence(&[
            add_counters(this(), CounterKind::Chorus, 1),
            draw(you(), 1),
            discard(you(), 1),
            if_then(
                Condition::Compare(
                    Amount::Counters(this(), CounterKind::Chorus),
                    Comparison::AtLeast,
                    Amount::Fixed(4),
                ),
                &may_cast_free(you(), those()),
            ),
        ]),
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MALCOLM: &str = Card::MalcolmAlluringScoundrel;

    /// A creature without flash: cast during the combat damage step, timing is ignored
    /// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    const SPELLBINDER: &str = Card::EliteSpellbinder;

    /// Malcolm attacks alone in `turn` and his trigger resolves; the discard has one kind of card
    /// to choose from.
    macro_rules! hit {
        ($turn:expr) => {
            [
                advance_to($turn, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(MALCOLM),
                advance_to($turn, TurnStep::CombatDamage),
                Resolve,
            ]
        };
    }

    const HITS: [[Action; 5]; 4] = [hit!(1), hit!(3), hit!(5), hit!(7)];

    pub const SCENARIOS: &[Scenario] = &[
        // Four hits: each loots, and the fourth discards the Spellbinder it drew and casts it free
        // in the combat damage step.
        Scenario::new("Malcolm's fourth chorus counter casts the discard")
            .battlefield(P0, &[MALCOLM])
            // Malcolm's draws in turns 1, 3, 5 and 7 and the draw steps of 3, 5 and 7: Islands,
            // then the Spellbinder last.
            .library(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    SPELLBINDER,
                ],
            )
            .library(P1, &[Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                HITS[0][0],
                HITS[0][1],
                HITS[0][2],
                HITS[0][3],
                HITS[0][4],
                ME.counters(MALCOLM, CounterKind::Chorus, 1),
                HITS[1][0],
                HITS[1][1],
                HITS[1][2],
                HITS[1][3],
                HITS[1][4],
                HITS[2][0],
                HITS[2][1],
                HITS[2][2],
                HITS[2][3],
                HITS[2][4],
                ME.counters(MALCOLM, CounterKind::Chorus, 3),
                HITS[3][0],
                HITS[3][1],
                HITS[3][2],
                HITS[3][3],
                HITS[3][4],
                // Three Islands and the Spellbinder in hand.
                ME.picks(SPELLBINDER),
                ME.chooses(DecisionKind::CastOffer, SPELLBINDER),
                stack(1),
                Resolve,
                ME.counters(MALCOLM, CounterKind::Chorus, 4),
                ME.controls(SPELLBINDER, 1),
                ME.has(Zone::Hand, Card::Island, 3),
                THEM.life(12),
            ]),
    ];
}
