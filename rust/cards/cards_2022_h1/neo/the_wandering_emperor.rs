// Generated from the card store by `just card header`: notes go below the end marker.
//! The Wandering Emperor {2}{W}{W}
//! Legendary Planeswalker, loyalty 3
//!
//! Flash
//! As long as The Wandering Emperor entered this turn, you may activate her loyalty abilities any
//! time you could cast an instant.
//! +1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of turn.
//! −1: Create a 2/2 white Samurai creature token with vigilance.
//! −2: Exile target tapped creature. You gain 2 life.
// End of the generated card header.

use crate::prelude::*;

/// The Samurai token's name ([CR 111.4]): "If the spell or ability doesn't specify the name of the
/// token, its name is the same as its subtype(s) plus the word 'Token.'"
///
/// [CR 111.4]: https://yawgatog.com/resources/magic-rules/#R1114
pub use crate::tokens::SAMURAI_TOKEN;

/// "A 2/2 white Samurai creature token with vigilance"
pub static SAMURAI: CardDef = Token::SamuraiToken.keywords(Keywords::VIGILANCE).card();

/// "You may activate her loyalty abilities any time you could cast an instant." Only "as long
/// as The Wandering Emperor entered this turn".
///
/// "+1: Put a +1/+1 counter on up to one target creature. It gains first strike until end of turn."
/// [CR 702.7a]: "First strike is a static ability that modifies the rules for the combat damage
/// step."
///
/// "−1: Create a 2/2 white Samurai creature token with vigilance." [CR 111.2]: "The player who
/// creates a token is its owner. The token enters the battlefield under that player's control."
///
/// "−2: Exile target tapped creature. You gain 2 life." [CR 701.13a]: "To exile an object, move it
/// to the exile zone from wherever it is."
///
/// [CR 702.7a]: https://yawgatog.com/resources/magic-rules/#R7027a
/// [CR 111.2]: https://yawgatog.com/resources/magic-rules/#R1112
/// [CR 701.13a]: https://yawgatog.com/resources/magic-rules/#R70113a
pub static CARD: CardDef = Printed::TheWanderingEmperor
    .keywords(Keywords::FLASH)
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::ActivateLoyaltyAbilities(Objects::This),
            when: When::AsInstant,
        })
        .only_if(&Condition::EnteredThisTurn(Objects::This)),
        Ability::loyalty(1)
            .target(&TargetSpec::creature().optional())
            .effect(&sequence(&[
                add_counters(target(), CounterKind::PlusOne, 1),
                modify(
                    target(),
                    Modification::AddKeywords(Keywords::FIRST_STRIKE),
                    Duration::EndOfTurn,
                ),
            ])),
        Ability::loyalty(-1).effect(&create_tokens(you(), &SAMURAI, 1)),
        Ability::loyalty(-2)
            .target(&TargetSpec::permanent(Filter::All(&[
                Filter::Types(Types::CREATURE),
                Filter::Tapped,
            ])))
            .effect(&sequence(&[exile(target()), gain_life(you(), 2)])),
    ])
    .card()
    .creating(&[&SAMURAI]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::SAMURAI_TOKEN;

    const EMPEROR: &str = Card::TheWanderingEmperor;

    // `Prop::SmallCreature` is a 2/1.
    pub const SCENARIOS: &[Scenario] = &[
        // Flashed in on the opponent's turn during combat, she activates at instant speed
        // (her static ability's rule change); the turn after, she waits for a main phase.
        Scenario::new("flashed in during combat, −2 exiles the tapped attacker")
            .at(2, P1, TurnStep::BeginningOfCombat)
            .hand(P0, &[EMPEROR])
            .battlefield(P0, &[Card::Plains; 4])
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.passes(),
                // The creature attacks, tapped; the attackers' controller has nothing to do.
                THEM.attacks_with(Prop::SmallCreature),
                // With nothing else to do, each resolves as soon as its controller passes.
                ME.casts(EMPEROR),
                ME.activates(EMPEROR, 3), // -2
                THEM.has(Zone::Exile, Prop::SmallCreature, 1),
                ME.life(22),
                ME.has_loyalty(EMPEROR, 1),
                // The turn after, in her controller's upkeep: no instant-speed loyalty abilities.
                advance_to(3, TurnStep::Upkeep),
                ME.deciding(DecisionKind::Priority, 1),
                advance_to(3, TurnStep::PrecombatMain),
                // A main phase with an empty stack: the land drawn and her two affordable
                // abilities (−2 has no tapped creature to target).
                ME.deciding(DecisionKind::Priority, 4),
            ]),
        Scenario::new("+1 grows a creature and gives it first strike")
            .battlefield(P0, &[EMPEROR])
            .battlefield(P1, &[Prop::SmallCreature])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(EMPEROR, 1), // +1
                ME.target_count(1),
                Resolve,
                THEM.pt(Prop::SmallCreature, 3, 2),
                ME.has_loyalty(EMPEROR, 4),
                // The counter stays after the turn.
                advance_to(2, TurnStep::Upkeep),
                THEM.pt(Prop::SmallCreature, 3, 2),
            ]),
        Scenario::new("−1 makes a 2/2 Samurai")
            .battlefield(P0, &[EMPEROR])
            .hand(P0, &[Card::Plains])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(EMPEROR, 2), // -1
                ME.token_pt(SAMURAI_TOKEN, 2, 2),
                ME.has_loyalty(EMPEROR, 2),
            ]),
    ];
}
