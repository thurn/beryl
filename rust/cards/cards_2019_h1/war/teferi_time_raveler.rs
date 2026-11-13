// Generated from the card store by `just card header`: notes go below the end marker.
//! Teferi, Time Raveler {1}{W}{U}
//! Legendary Planeswalker — Teferi, loyalty 4
//!
//! Each opponent can cast spells only any time they could cast a sorcery.
//! +1: Until your next turn, you may cast sorcery spells as though they had flash.
//! −3: Return up to one target artifact, creature, or enchantment to its owner's hand. Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// Planeswalkers' loyalty abilities ([CR 606.3](https://yawgatog.com/resources/magic-rules/#R6063)):
/// "A player may activate a loyalty ability of a permanent they control any time they have
/// priority and the stack is empty during a main phase of their turn, but only if no player
/// has previously activated a loyalty ability of that permanent that turn."
///
/// "Each opponent can cast spells only any time they could cast a sorcery."
///
/// "You may cast sorcery spells as though they had flash."
/// [CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a): "'Flash' means 'You may
/// play this card any time you could cast an instant.'" A change to a rule for one player
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous effects
/// affect game rules rather than objects.").
pub static CARD: CardDef = Printed::TeferiTimeRaveler
    .abilities(&[
        // A prohibition the casting machine reads: a "can't" beats a "can"
        // ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)), so even a
        // spell with flash waits for sorcery timing.
        Ability::rule_change(&RuleChange {
            players: Players::EachOpponent,
            what: Governed::CastSpells(Filter::Any),
            when: When::OnlyAsSorcery,
        }),
        // "+1: Until your next turn, you may cast sorcery spells as though they had
        // flash." It ends as its controller's next turn begins.
        Ability::loyalty(1).effect(&change_rule(
            &RuleChange {
                players: Players::You,
                what: Governed::CastSpells(Filter::Types(Types::SORCERY)),
                when: When::AsInstant,
            },
            Duration::UntilYourNextTurn,
        )),
        // "−3: Return up to one target artifact, creature, or enchantment to its owner's
        // hand. Draw a card." With no target chosen it still draws; with its one target
        // illegal, it doesn't resolve at all
        // ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)).
        Ability::loyalty(-3)
            .target(
                &TargetSpec::permanent(Filter::Types(
                    Types::ARTIFACT
                        .union(Types::CREATURE)
                        .union(Types::ENCHANTMENT),
                ))
                .optional(),
            )
            .effect(&sequence(&[return_to_hand(target()), draw(you(), 1)])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TEFERI: &str = Card::TeferiTimeRaveler;

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A planeswalker or battle
        // that’s removed from combat stops being attacked." Teferi's −3, with no target, leaves
        // him 1 loyalty; in P0's turn the 3/1 removes him, and the 2/1's attack on the player
        // goes on. The other planeswalker is a destination no one picks.
        Scenario::new("combat damage removes Teferi while other attacks remain")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Prop::Creature, Prop::SmallCreature])
            .battlefield(P1, &[TEFERI, Prop::Planeswalker])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                THEM.activates(TEFERI, 2), // −3
                THEM.target_count(0),
                THEM.has_loyalty(TEFERI, 1),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.chooses(DecisionKind::AttackTarget, TEFERI),
                ME.attacks_with(Prop::SmallCreature),
                ME.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
                advance_to(3, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, TEFERI, 1),
                THEM.has_loyalty(Prop::Planeswalker, 3),
                THEM.life(18),
            ]),
        // CR 506.4 again, with the 2/1 the last attacker declared and sent at the other
        // planeswalker: its attack goes on after Teferi leaves, and its line begins as it is
        // declared (by-w8gv.21.57).
        Scenario::new("combat damage removes Teferi while the other planeswalker is attacked")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Prop::Creature, Prop::SmallCreature])
            .battlefield(P1, &[TEFERI, Prop::Planeswalker])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                THEM.activates(TEFERI, 2), // −3
                THEM.target_count(0),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Creature),
                ME.chooses(DecisionKind::AttackTarget, TEFERI),
                ME.attacks_with(Prop::SmallCreature),
                ME.chooses(DecisionKind::AttackTarget, Prop::Planeswalker),
                advance_to(3, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, TEFERI, 1),
                THEM.has_loyalty(Prop::Planeswalker, 1),
                THEM.life(20),
            ]),
        // CR 101.2 and 307.1: a flash spell in an opponent's hand waits for their own main phase
        // with an empty stack.
        Scenario::new("an opponent's flash spell waits for sorcery timing")
            .battlefield(P0, &[TEFERI])
            .hand(P1, &[Prop::SmallCreature])
            .battlefield(P1, &[Card::Island, Card::Island])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                // Only passing in P1's upkeep: the creature has flash, but Teferi forbids it now.
                advance_to(2, TurnStep::Upkeep),
                THEM.deciding(DecisionKind::Priority, 1),
                // The stack empty in its main phase: sorcery timing.
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Prop::SmallCreature),
                Resolve,
                THEM.controls(Prop::SmallCreature, 1),
            ]),
        Scenario::new("−3 returns an opposing artifact and draws a card")
            .battlefield(P0, &[TEFERI])
            .battlefield(P1, &[Prop::Artifact])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(TEFERI, 2), // −3
                ME.target_count(1),
                THEM.has(Zone::Hand, Prop::Artifact, 1),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_loyalty(TEFERI, 1),
                // CR 606.3: one loyalty ability a turn.
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        Scenario::new("+1 lets a sorcery be cast on the opponent's turn, until the next")
            .battlefield(P0, &[TEFERI, Card::Plains])
            .hand(P0, &[Card::Oust, Card::Oust])
            .battlefield(P1, &[Prop::Creature])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                ME.activates(TEFERI, 1), // +1
                Resolve,
                advance_to(2, TurnStep::Upkeep),
                THEM.passes(),
                // In the opponent's upkeep, as though it had flash (CR 702.8a).
                ME.casts(Card::Oust),
                // Second from the top: P1 has drawn the Plains above it since, and stops in their
                // main phase with it to play.
                THEM.has(Zone::Hand, Card::Plains, 1),
                THEM.library_top(&[Prop::Creature, Card::Island]),
                THEM.life(23),
                // "Until your next turn": in its controller's next upkeep the other Oust waits for
                // a main phase.
                advance_to(3, TurnStep::Upkeep),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
    ];
}
