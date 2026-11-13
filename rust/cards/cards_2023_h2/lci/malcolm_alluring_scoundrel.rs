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

use crate::{names, printed};
use beryl_card::effect::build::{
    this, those, you, add_counters, discard, draw, if_then, may_cast_free, sequence,
};
use beryl_card::{
    Ability, Amount, CardDef, Comparison, Condition, CounterKind, Effect, Keywords, Trigger, card,
};

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
static CHORUS: Effect = sequence(&[
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
]);

pub static CARD: CardDef = card(
    names::MALCOLM_ALLURING_SCOUNDREL,
    &[printed::MALCOLM_ALLURING_SCOUNDREL
        .keywords(Keywords::FLASH.union(Keywords::FLYING))
        .abilities(&[
            Ability::triggered(&Trigger::DEALS_COMBAT_DAMAGE_TO_A_PLAYER).effect(&CHORUS),
        ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const MALCOLM: &str = names::MALCOLM_ALLURING_SCOUNDREL;

    /// A creature without flash: cast during the combat damage step, timing is ignored
    /// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    /// Malcolm attacks alone in `turn` and his trigger resolves; the discard has one kind of card
    /// to choose from.
    macro_rules! hit {
        ($turn:expr) => {
            [
                Action::AdvanceTo {
                    turn: $turn,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Card { card: MALCOLM },
                },
                Action::AdvanceTo {
                    turn: $turn,
                    step: TurnStep::CombatDamage,
                },
                Action::Resolve,
            ]
        };
    }

    const HITS: [[Action; 5]; 4] = [hit!(1), hit!(3), hit!(5), hit!(7)];

    const fn chorus_counters(count: u8) -> Action {
        Action::Assert {
            assertion: Assertion::Counters {
                player: P0,
                card: MALCOLM,
                kind: CounterKind::Chorus,
                count,
            },
        }
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Four hits: each loots, and the fourth discards the Spellbinder it drew and casts it free
        // in the combat damage step.
        Scenario::new("Malcolm's fourth chorus counter casts the discard")
            .permanents(P0, &[Permanent::new(MALCOLM)])
            // Malcolm's draws in turns 1, 3, 5 and 7 and the draw steps of 3, 5 and 7: Islands,
            // then the Spellbinder last.
            .library(
                P0,
                &[
                    names::ISLAND,
                    names::ISLAND,
                    names::ISLAND,
                    names::ISLAND,
                    names::ISLAND,
                    names::ISLAND,
                    SPELLBINDER,
                ],
            )
            .library(P1, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .actions(&[
                HITS[0][0],
                HITS[0][1],
                HITS[0][2],
                HITS[0][3],
                HITS[0][4],
                chorus_counters(1),
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
                chorus_counters(3),
                HITS[3][0],
                HITS[3][1],
                HITS[3][2],
                HITS[3][3],
                HITS[3][4],
                // Three Islands and the Spellbinder in hand.
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    pick: Pick::Card { card: SPELLBINDER },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::CastOffer,
                    pick: Pick::Card { card: SPELLBINDER },
                },
                Action::Assert {
                    assertion: Assertion::Stack { count: 1 },
                },
                Action::Resolve,
                chorus_counters(4),
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: SPELLBINDER,
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P0,
                        zone: Zone::Hand,
                        card: Some(names::ISLAND),
                        count: 3,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Life {
                        player: P1,
                        life: 12,
                    },
                },
            ]),
    ];
}
