// Generated from the card store by `just card header`: notes go below the end marker.
//! Get Lost {1}{W}
//! Instant
//!
//! Destroy target creature, enchantment, or planeswalker. Its controller creates two Map tokens.
//! (They're artifacts with "{1}, {T}, Sacrifice this token: Target creature you control explores.
//! Activate only as a sorcery.")
// End of the generated card header.

use crate::prelude::*;

/// "Its controller" is the permanent's controller as it was destroyed
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect requires
/// information from the game ..., the answer is determined only once, when the effect is
/// applied. If the effect requires information from a specific object, ... the effect uses
/// the current information of that object if it's in the public zone it was expected to be
/// in; if it's no longer in that zone, ... the effect uses the object's last known
/// information."
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

/// The Map token's name: its subtype plus the word "Token" (CR 111.4).
pub use crate::tokens::MAP_TOKEN;

/// The predefined Map token
/// ([CR 111.10s](https://yawgatog.com/resources/magic-rules/#R11110s)): "A Map token is a
/// colorless Map artifact token with "{1}, {T}, Sacrifice this token: Target creature you
/// control explores. Activate only as a sorcery.""
/// Explore is [CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a).
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

    const SPELLBINDER: &str = Card::EliteSpellbinder;

    const MAP_NAME: &str = MAP.name;

    /// In P1's turn 2, P0 destroys one of P1's two Elite Spellbinders with Get Lost at instant
    /// speed; P1 then sacrifices a Map, paying {1} with their Plains, and their other Spellbinder
    /// explores the top of `top`. The Island in P1's hand keeps their priority a decision.
    const fn exploring(name: &'static str, top: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::GetLost])
            .library(P0, &[Card::Plains])
            .battlefield(P1, &[SPELLBINDER, SPELLBINDER, Card::Plains])
            .hand(P1, &[Card::Island])
            .library(P1, top)
    }

    const LET_P0_ACT: Action = THEM.passes();

    const GET_LOST: Action = ME.casts(Card::GetLost);

    const SACRIFICE_A_MAP: Action = THEM.activates(MAP_NAME, 0);

    pub const SCENARIOS: &[Scenario] = &[
        // With no legal target it can't be cast
        // ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's Mana Crypt is
        // an artifact, so passing is P0's only option.
        Scenario::new("Get Lost can't be cast without a target")
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::GetLost])
            .battlefield(P1, &[Prop::Artifact])
            .actions(&[
                advance_to(1, TurnStep::PrecombatMain),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        // Its mana cost is {1}{W}: two Plains cast it. The oracles caught it costing {1}{W}{W}.
        Scenario::new("Get Lost costs one and a white")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost, Card::Island])
            .battlefield(P1, &[SPELLBINDER])
            .actions(&[
                ME.casts(Card::GetLost),
                THEM.has(Zone::Graveyard, SPELLBINDER, 1),
                ME.tapped(Card::Plains, 2),
            ]),
        // The Island left in P0's hand keeps the game in P0's main phase once Get Lost has
        // resolved.
        Scenario::new("Get Lost on a planeswalker gives its controller two Maps")
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::GetLost, Card::Island])
            .battlefield(P1, &[Card::JaceTheMindSculptor])
            .actions(&[
                ME.casts(Card::GetLost),
                THEM.has(Zone::Graveyard, Card::JaceTheMindSculptor, 1),
                THEM.has(Zone::Battlefield, MAP_NAME, 2),
                ME.has(Zone::Battlefield, MAP_NAME, 0),
            ]),
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
            THEM.has(Zone::Graveyard, SPELLBINDER, 1),
            THEM.has(Zone::Battlefield, MAP_NAME, 2),
            ME.has(Zone::Battlefield, MAP_NAME, 0),
            SACRIFICE_A_MAP,
            THEM.has(Zone::Battlefield, MAP_NAME, 1),
            THEM.has(Zone::Hand, Card::Plains, 1),
            THEM.library_top(&[Card::Island]),
            THEM.counters(SPELLBINDER, CounterKind::PlusOne, 0),
        ]),
        exploring(
            "A Map explores a nonland into a counter and the graveyard",
            &[Card::Oust, Card::Island],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            THEM.yes(),
            THEM.counters(SPELLBINDER, CounterKind::PlusOne, 1),
            THEM.has(Zone::Graveyard, Card::Oust, 1),
            THEM.library_top(&[Card::Island]),
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
            THEM.counters(SPELLBINDER, CounterKind::PlusOne, 1),
            THEM.has(Zone::Graveyard, Card::Oust, 0),
            THEM.library_top(&[Card::Oust, Card::Island]),
        ]),
        exploring("A Map explores an empty library into a counter", &[]).actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            THEM.counters(SPELLBINDER, CounterKind::PlusOne, 1),
        ]),
    ];
}
