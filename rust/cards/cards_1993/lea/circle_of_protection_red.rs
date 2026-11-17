// Generated from the card store by `just card header`: notes go below the end marker.
//! Circle of Protection: Red {1}{W}
//! Enchantment
//!
//! {1}: The next time a red source of your choice would deal damage to you this turn, prevent that
//! damage.
// End of the generated card header.

use crate::prelude::*;

/// "{1}: The next time a red source of your choice would deal damage to you this turn, prevent
/// that damage." Its controller chooses the source as the ability resolves, and the source is not
/// a target ([CR 609.7a]): "If an effect requires a player to choose a source of damage, they may
/// choose a permanent; a spell on the stack (including a permanent spell)". The shield prevents
/// all the damage of the next event from that source, then is used up ([CR 615.8]): "These
/// effects prevent the next instance of damage from that source, regardless of how much damage
/// that is."
///
/// [CR 609.7a]: https://yawgatog.com/resources/magic-rules/#R6097a
/// [CR 615.8]: https://yawgatog.com/resources/magic-rules/#R6158
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
        // Chosen as P1's Hill Giant attacks, the shield prevents its 3 combat damage to P0.
        Scenario::new("Circle of Protection: Red prevents a red attacker's combat damage")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[COP, Card::Plains])
            .battlefield(P1, &[GIANT])
            .actions(&[
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.attacks_with(GIANT),
                ME.activates(COP, 0),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(20),
                ME.tapped(Card::Plains, 1),
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
    ];
}
