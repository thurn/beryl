// Generated from the card store by `just card header`: notes go below the end marker.
//! Mother of Runes {W}
//! Creature — Human Cleric, 1/1
//!
//! {T}: Target creature you control gains protection from the color of your choice until end of
//! turn.
// End of the generated card header.

use crate::prelude::*;

/// "{T}: Target creature you control gains protection from the color of your choice until end of
/// turn." Its controller chooses the color as the ability resolves ([CR 608.2d]): "If an effect
/// of a spell or ability offers any choices other than choices already made as part of casting
/// the spell, activating the ability, or otherwise putting the spell or ability on the stack,
/// the player announces these while applying the effect." Protection ([CR 702.16]) keeps the
/// creature from being targeted, enchanted, equipped and blocked by what has the color, and
/// prevents damage from sources of it.
///
/// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
/// [CR 702.16]: https://yawgatog.com/resources/magic-rules/#R70216
pub static CARD: CardDef = Printed::MotherOfRunes
    .abilities(&[Ability::activated(Cost::TAP)
        .target(&TargetSpec::permanent(Filter::All(&[
            Filter::Types(Types::CREATURE),
            Filter::Whose(&Players::You),
        ])))
        .effect(&modify(
            target(),
            &[Modification::AddProtection(Protection::ChosenColor)],
            Duration::EndOfTurn,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MOTHER: &str = Card::MotherOfRunes;
    const BOLT: &str = Card::LightningBolt;
    const HOLY: &str = Card::HolyStrength;

    // `Prop::Creature` is a white 3/1 flier, `Prop::SmallCreature` a blue 2/1.

    /// P0 holds Lightning Bolt and a Mountain to cast it, Mother of Runes and a white
    /// creature.
    const BOLTING: Scenario = Scenario::setup()
        .interaction(&[BOLT])
        .battlefield(P0, &[Card::Mountain, MOTHER, Prop::Creature])
        .hand(P0, &[BOLT, Card::Mountain]);

    /// P0 casts Lightning Bolt at their own white creature and, in response, activates
    /// Mother of Runes targeting it.
    const BOLT_IT: [Action; 4] = [
        ME.casts(BOLT),
        ME.targets(Prop::Creature),
        ME.activates(MOTHER, 0),
        ME.targets(Prop::Creature),
    ];

    /// In P0's turn 3, P0 attacks with their blue 2/1 into P1's white 3/1.
    const fn attacking(name: &'static str) -> Scenario {
        Scenario::new(name)
            .at(3, P0, TurnStep::PrecombatMain)
            .battlefield(P0, &[MOTHER, Prop::SmallCreature])
            .battlefield(P1, &[Prop::Creature])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
    }

    pub const SCENARIOS: &[Scenario] = &[
        // [CR 702.16b]: "A permanent or player with protection can’t be targeted by spells
        // with the stated quality". Its one target illegal, the spell does nothing
        // ([CR 608.2b]).
        //
        // [CR 702.16b]: https://yawgatog.com/resources/magic-rules/#R70216b
        // [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
        BOLTING
            .titled("protection from red makes Lightning Bolt do nothing")
            .actions(&[
                BOLT_IT[0],
                BOLT_IT[1],
                BOLT_IT[2],
                BOLT_IT[3],
                ME.chooses_color(Colors::RED),
                ME.has(Zone::Graveyard, BOLT, 1),
                ME.has(Zone::Battlefield, Prop::Creature, 1),
                ME.tapped(MOTHER, 1),
            ]),
        BOLTING
            .titled("protection from blue doesn't stop Lightning Bolt")
            .actions(&[
                BOLT_IT[0],
                BOLT_IT[1],
                BOLT_IT[2],
                BOLT_IT[3],
                ME.chooses_color(Colors::BLUE),
                ME.has(Zone::Graveyard, Prop::Creature, 1),
            ]),
        // "You control": P1's creature is no target, so Mother and P0's creature are the
        // two.
        Scenario::new("Mother of Runes targets only a creature its controller controls")
            .battlefield(P0, &[MOTHER, Prop::Creature])
            .battlefield(P1, &[Prop::SmallCreature])
            .hand(P0, &[Card::Island])
            .actions(&[
                ME.activates(MOTHER, 0),
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(Prop::Creature),
                ME.chooses_color(Colors::GREEN),
                ME.tapped(MOTHER, 1),
            ]),
        // [CR 702.16c]: "Such Auras attached to the permanent or player with protection will
        // be put into their owners’ graveyards as a state-based action."
        //
        // [CR 702.16c]: https://yawgatog.com/resources/magic-rules/#R70216c
        Scenario::new("protection from white makes a white Aura fall off")
            .interaction(&[HOLY])
            .battlefield(P0, &[Card::Plains, MOTHER, Prop::SmallCreature])
            .hand(P0, &[HOLY, Card::Island])
            .actions(&[
                ME.casts(HOLY),
                ME.targets(Prop::SmallCreature),
                Resolve,
                ME.pt(Prop::SmallCreature, 3, 3),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::SmallCreature),
                ME.chooses_color(Colors::WHITE),
                ME.has(Zone::Graveyard, HOLY, 1),
                ME.pt(Prop::SmallCreature, 2, 1),
            ]),
        // [CR 702.16f]: "Attacking creatures with protection can’t be blocked by creatures
        // that have the stated quality." P1's white creature can't block, so P1 takes 2.
        //
        // [CR 702.16f]: https://yawgatog.com/resources/magic-rules/#R70216f
        attacking("an attacker with protection from white can't be blocked by a white creature")
            .actions(&[
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::SmallCreature),
                ME.done_attacking(),
                ME.activates(MOTHER, 0),
                ME.targets(Prop::SmallCreature),
                ME.chooses_color(Colors::WHITE),
                advance_to(3, TurnStep::EndOfCombat),
                THEM.life(18),
                ME.has(Zone::Battlefield, Prop::SmallCreature, 1),
            ]),
        // [CR 702.16e]: "Any damage that would be dealt by sources that have the stated
        // quality to a permanent or player with protection is prevented." Blocked first, the
        // attacker stays blocked; the white blocker's 3 damage is prevented, and the attacker's
        // 2 kill it.
        //
        // [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
        attacking("protection from white prevents a white blocker's combat damage").actions(&[
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(Prop::SmallCreature),
            ME.done_attacking(),
            ME.passes(),
            THEM.chooses(DecisionKind::Blocker, Prop::Creature),
            ME.activates(MOTHER, 0),
            ME.targets(Prop::SmallCreature),
            ME.chooses_color(Colors::WHITE),
            advance_to(3, TurnStep::EndOfCombat),
            ME.has(Zone::Battlefield, Prop::SmallCreature, 1),
            THEM.has(Zone::Graveyard, Prop::Creature, 1),
            THEM.life(20),
        ]),
    ];
}
