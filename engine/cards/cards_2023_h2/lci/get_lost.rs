// Generated from the card store by `just card header`: notes go below the end marker.
//! Get Lost {1}{W}
//! Instant
//!
//! Destroy target creature, enchantment, or planeswalker. Its controller creates two Map tokens.
//! (They're artifacts with "{1}, {T}, Sacrifice this token: Target creature you control explores.
//! Activate only as a sorcery.")
// End of the generated card header.

use crate::prelude::*;

/// The Maps go to the permanent's last battlefield controller. [CR 608.2h] says “the
/// effect uses the object's last known information” when the object has left the
/// expected public zone.
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
pub static CARD: CardDef = Printed::GetLost
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(Filter::Types(
            Types::CREATURE
                .union(Types::ENCHANTMENT)
                .union(Types::PLANESWALKER),
        )))
        .effect(&sequence(&[
            destroy(target()),
            create_tokens(its_controller(), &MAP, 2),
        ]))])
    .card()
    .creating(&[&MAP]);

pub use crate::tokens::MAP_TOKEN;

pub static MAP: CardDef = Token::MapToken
    .abilities(&[Ability::activated(Cost::of(
        ManaCost::generic(1),
        &[CostPart::TAP, CostPart::SACRIFICE],
    ))
    .target(&TargetSpec::permanent(Filter::All(&[
        Filter::Types(Types::CREATURE),
        Filter::Whose(&Players::You),
    ])))
    .as_a_sorcery()
    .effect(&explore(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::MAP;

    const FLIER: &str = Prop::Flier;

    const MAP_NAME: &str = MAP.name;

    /// In P1's turn 2, P0 destroys one of P1's two fliers with Get Lost at instant
    /// speed; P1 then sacrifices a Map, paying {1} with their Plains, and their other flier
    /// explores the top of `top`. The Island in P1's hand keeps their priority a decision.
    const fn exploring(name: &'static str, top: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::GetLost])
            .library(P0, &[Card::Plains])
            .battlefield(P1, &[FLIER, FLIER, Card::Plains])
            .hand(P1, &[Card::Island])
            .library(P1, top)
    }

    const LET_P0_ACT: Action = THEM.passes();

    const GET_LOST: Action = ME.casts(Card::GetLost);

    const SACRIFICE_A_MAP: Action = THEM.activates(MAP_NAME, 0);

    pub const SCENARIOS: &[Scenario] = &[
        // Explore ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): "...that
        // permanent's controller reveals the top card of their library. If a land card is
        // revealed this way, that player puts that card into their hand. Otherwise, that player
        // puts a +1/+1 counter on the exploring permanent and may put the revealed card into
        // their graveyard." With nothing to reveal, no land is revealed, so the creature gets
        // the counter.
        exploring(
            "Get Lost on a creature gives two Maps; one explores a land into hand",
            &[Card::Plains, Card::Island],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            THEM.has(Zone::Graveyard, FLIER, 1),
            THEM.has(Zone::Battlefield, MAP_NAME, 2),
            ME.has(Zone::Battlefield, MAP_NAME, 0),
            SACRIFICE_A_MAP,
            THEM.has(Zone::Battlefield, MAP_NAME, 1),
            THEM.has(Zone::Hand, Card::Plains, 1),
            THEM.library_top(&[Card::Island]),
            THEM.counters(FLIER, CounterKind::PlusOne, 0),
        ]),
        exploring(
            "A Map explores a nonland into a counter, left on top",
            &[Card::Oust, Card::Island],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            THEM.no(),
            THEM.counters(FLIER, CounterKind::PlusOne, 1),
            THEM.has(Zone::Graveyard, Card::Oust, 0),
            THEM.library_top(&[Card::Oust, Card::Island]),
        ]),
    ];
}
