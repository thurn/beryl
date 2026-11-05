//! Oust. {W} Sorcery. "Put target creature into its owner's library second from the top. Its
//! controller gains 3 life."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::OUST,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(Types::CREATURE),
            )],
            body: oust,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Second from the top" is under one card, or the top of an empty library. "Its controller"
/// is the creature's as it was on the battlefield: in a library it has no controller
/// ([CR 108.4a](https://yawgatog.com/resources/magic-rules/#R1084a)): "If anything asks for
/// the controller of a card that doesn't have one (because it's not a permanent or spell),
/// use its owner instead." Owner and controller differ only for a stolen creature, which
/// goes to its owner's library while its controller gains the life.
fn oust(ctx: &mut EffectCtx) -> Flow {
    let Some(creature) = ctx.target_object(0) else {
        return Ok(());
    };
    let controller = ctx.state().controller(creature.slot);
    ctx.put_into_library(creature, 1);
    ctx.gain_life(controller, 3);
    Ok(())
}

/// With no legal target it can't be cast
/// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's Mana Crypt is an
/// artifact, so passing is P0's only option.
pub const SCENARIOS: &[Scenario] = &[Scenario::new("Oust can't be cast without a target")
    .battlefield(
        P0,
        &[
            Permanent::new(names::PLAINS),
            Permanent::new(names::PLAINS),
            Permanent::new(names::PLAINS),
        ],
    )
    .hand(P0, &[names::OUST])
    .battlefield(P1, &[Permanent::new(names::MANA_CRYPT)])
    .library(P1, &[names::ISLAND])
    .actions(&[
        Action::AdvanceTo {
            turn: 1,
            step: TurnStep::PrecombatMain,
        },
        Action::assert(Assertion::Decision {
            player: P0,
            kind: DecisionKind::Priority,
            candidates: 1,
        }),
    ])];
