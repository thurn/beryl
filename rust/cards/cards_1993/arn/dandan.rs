// Generated from the card store by `just card header`: notes go below the end marker.
//! Dandân {U}{U}
//! Creature — Fish, 4/1
//!
//! This creature can't attack unless defending player controls an Island.
//! When you control no Islands, sacrifice this creature.
// End of the generated card header.

use crate::prelude::*;

/// "This creature can't attack unless defending player controls an Island." is a restriction
/// on declaring it as an attacker ([CR 508.1c]): "The active player checks each creature they
/// control to see whether it's affected by any restrictions (effects that say a creature
/// can't attack, or that it can't attack unless some condition is met)." It is read for each
/// player it could attack.
///
/// "When you control no Islands, sacrifice this creature." is a state trigger ([CR 603.8]):
/// "These abilities trigger as soon as the game state matches the condition. ... A
/// state-triggered ability doesn't trigger again until the ability has resolved, has been
/// countered, or has otherwise left the stack."
///
/// "Island" in both abilities is a land type word, which a text-changing effect can change
/// ([CR 612.2]): "A text-changing effect changes only those words that are used in the
/// correct way (for example, a Magic color word being used as a color word, a land type word
/// used as a land type, or a creature type word used as a creature type)." With "Island"
/// read as "Swamp", Dandân attacks only a player who controls a Swamp, and is sacrificed when
/// its controller controls no Swamp.
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
/// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
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
            Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Subtype(Subtype::Island),
                    Filter::Whose(&Players::You),
                ]),
            }),
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

    // `Prop::Creature` is Elite Spellbinder, a 3/1 flier, there so that P0 declares
    // attackers even when Dandân can't attack. That a text change lets Dandân attack a player
    // with a Swamp and no Island is a test of `beryl_cards`, `tests::text`: Forge changes the
    // word in the trigger but not in the attack restriction, and XMage has no Mind Bend or
    // Crystal Spray, so no oracle can show it.
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
        // P1 controls no Island, so only the Spellbinder is offered, beside "done".
        Scenario::new("Dandân can't attack a player who controls no Island")
            .battlefield(P0, &[DANDAN, Card::Island, Prop::Creature])
            .battlefield(P1, &[Card::Plains])
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
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
        // "Island" reads "Swamp", so P1's Island no longer lets Dandân attack.
        Scenario::new("Mind Bend keeps Dandân from attacking a player with only an Island")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[DANDAN, Card::Island, Card::Swamp, Prop::Creature])
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
    ];
}
