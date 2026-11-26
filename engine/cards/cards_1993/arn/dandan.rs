// Generated from the card store by `just card header`: notes go below the end marker.
//! Dandân {U}{U}
//! Creature — Fish, 4/1
//!
//! This creature can't attack unless defending player controls an Island.
//! When you control no Islands, sacrifice this creature.
// End of the generated card header.

use crate::prelude::*;

/// "Island" is a land type word in both abilities ([CR 612.2]): "A text-changing effect
/// changes only those words that are used in the correct way". Mind Bend can change which
/// land type the defending player needs and which land type keeps Dandân on the battlefield.
///
/// [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122
pub static CARD: CardDef = Printed::Dandan
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::Attack {
                attackers: Objects::This,
                unless_defender_controls: Some(Filter::Subtype(Subtype::Island)),
            },
            when: When::Never,
        }),
        Ability::triggered(&Trigger::state(&Condition::Compare(
            Amount::Count(&permanents(Filter::All(&[
                Filter::Subtype(Subtype::Island),
                yours(),
            ]))),
            Comparison::Equal,
            Amount::Fixed(0),
        )))
        .effect(&sacrifice(this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DANDAN: &str = Card::Dandan;
    const MIND_BEND: &str = Card::MindBend;
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
    const SWAMP: TextWord = TextWord::LandType(Subtype::Swamp);

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Dandân attacks a player who controls an Island")
            .battlefield(P0, &[DANDAN, Card::Island])
            .battlefield(P1, &[Card::Island])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(DANDAN),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(16),
            ]),
        // P0 taps their one Island for Mind Bend, which makes it a Swamp: P0 controls no
        // Island, so Dandân's trigger sacrifices it.
        Scenario::new("Dandân is sacrificed when its controller controls no Island")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[DANDAN, Card::Island])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::Island),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(SWAMP),
                Resolve,
                ME.has(Zone::Graveyard, DANDAN, 1),
            ]),
        // "Island" reads "Swamp", and P0 controls an Island but no Swamp.
        Scenario::new("Mind Bend has Dandân sacrificed for want of a Swamp")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[DANDAN, Card::Island])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(DANDAN),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(SWAMP),
                Resolve,
                ME.has(Zone::Graveyard, DANDAN, 1),
            ]),
        // A flier keeps the attacker decision available after Mind Bend disqualifies Dandân.
        Scenario::new("Mind Bend keeps Dandân from attacking a player with only an Island")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[DANDAN, Card::Island, Card::Swamp, Prop::Flier])
            .battlefield(P1, &[Card::Island])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(DANDAN),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(SWAMP),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 2),
            ]),
        Scenario::new("Dandân attacks in a shared game when the defender controls an Island")
            .shared_library(&[Card::Island, Card::Island])
            .battlefield(P0, &[DANDAN, Card::Island])
            .battlefield(P1, &[Card::Island])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(DANDAN),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(16),
            ]),
        Scenario::new(
            "Dandân is sacrificed to the shared graveyard when its controller controls no Island",
        )
        .shared_library(&[Card::Island, Card::Island])
        .interaction(&[MIND_BEND])
        .battlefield(P0, &[DANDAN, Card::Island])
        .hand(P0, &[MIND_BEND])
        .actions(&[
            ME.casts(MIND_BEND),
            ME.targets(Card::Island),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            Resolve,
            ME.has(Zone::Graveyard, DANDAN, 1),
            THEM.has(Zone::Graveyard, DANDAN, 1),
        ]),
    ];
}
