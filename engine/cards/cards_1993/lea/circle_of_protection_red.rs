// Generated from the card store by `just card header`: notes go below the end marker.
//! Circle of Protection: Red {1}{W}
//! Enchantment
//!
//! {1}: The next time a red source of your choice would deal damage to you this turn, prevent that
//! damage.
// End of the generated card header.

use crate::prelude::*;

/// The source choice can name a spell that will become a permanent. The shield follows that
/// permanent's damage ([CR 609.7a]): "If the player chooses a permanent spell, the effect
/// will apply to any damage dealt by that spell and any damage dealt by the permanent that
/// spell becomes when it resolves."
///
/// [CR 609.7a]: https://yawgatog.com/resources/magic-rules/#R6097a
pub static CARD: CardDef = Printed::CircleOfProtectionRed
    .abilities(&[Ability::activated(Cost::mana(mana("{1}"))).effect(
        &prevent_next_damage_from_chosen_source(Players::You, &Filter::Colors(Colors::RED)),
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const COP: &str = Card::CircleOfProtectionRed;
    const BOLT: &str = Card::LightningBolt;
    const GIANT: &str = Card::HillGiant;

    pub const SCENARIOS: &[Scenario] = &[
        // The red sources are P1's Hill Giant and the Bolt on the stack: "A source of damage is
        // a permanent, a spell on the stack (including one that creates a permanent), or any
        // object referred to by an object on the stack." P0 chooses the Bolt.
        Scenario::new("Circle of Protection: Red prevents Lightning Bolt's damage")
            .interaction(&[BOLT])
            .battlefield(P0, &[COP, Card::Plains, Card::Mountain])
            .battlefield(P1, &[GIANT])
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(ME),
                ME.activates(COP, 0),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(BOLT),
                Resolve,
                ME.life(20),
                ME.has(Zone::Graveyard, BOLT, 1),
            ]),
        // "To you": the shield is on its controller alone, so P0's own Hill Giant, the one red
        // source, still deals its 3 combat damage to P1.
        Scenario::new("Circle of Protection: Red prevents damage only to its controller")
            .battlefield(P0, &[COP, Card::Plains, GIANT])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.activates(COP, 0),
                Resolve,
                ME.passes(),
                ME.attacks_with(GIANT),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(17),
            ]),
        // The shield can be set up before an attacker is declared and still prevent combat
        // damage from the chosen source later in the turn.
        Scenario::new("Circle of Protection: Red shields against a creature before it attacks")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[COP, Card::Plains])
            .battlefield(P1, &[GIANT])
            .actions(&[
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.activates(COP, 0),
                THEM.attacks_with(GIANT),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(20),
            ]),
        // A red permanent that cannot deal damage is still a legal choice.
        Scenario::new("Circle of Protection: Red can choose Blood Moon")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[COP, Card::Plains])
            .battlefield(P1, &[Card::BloodMoon, GIANT])
            .actions(&[
                ME.activates(COP, 0),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::BloodMoon),
                ME.tapped(Card::Plains, 1),
            ]),
        // Snapcaster Mage's trigger names the Bolt while it's still a card in the graveyard.
        Scenario::new("Circle of Protection: Red can choose a Bolt referred to by a trigger")
            .interaction(&[BOLT, Card::SnapcasterMage])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[COP, Card::Plains])
            .battlefield(P1, &[GIANT, Card::Island, Card::Island])
            .hand(P1, &[Card::SnapcasterMage])
            .graveyard(P1, &[BOLT])
            .actions(&[
                THEM.casts(Card::SnapcasterMage),
                Resolve,
                THEM.passes(),
                ME.activates(COP, 0),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(BOLT),
                Resolve,
                THEM.has(Zone::Graveyard, BOLT, 1),
            ]),
        // Choose the Goblin while it's a spell. It can attack that turn because it has haste.
        Scenario::new("Circle of Protection: Red shields against the permanent a spell becomes")
            .interaction(&[Card::RagingGoblin])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[COP, Card::Plains])
            .battlefield(P1, &[GIANT, Card::Mountain])
            .hand(P1, &[Card::RagingGoblin])
            .actions(&[
                THEM.casts(Card::RagingGoblin),
                ME.activates(COP, 0),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::RagingGoblin),
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Card::RagingGoblin),
                THEM.done_attacking(),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(20),
            ]),
    ];
}
