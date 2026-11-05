//! Get Lost. {1}{W}{W} Instant. "Destroy target creature, enchantment, or planeswalker. Its
//! controller creates two Map tokens."
//!
//! Map: Token Artifact — Map. "{1}, {T}, Sacrifice this token: Target creature you control
//! explores. Activate only as a sorcery."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Cost, Face, Flow, ManaCost, Subtype, Subtypes,
    TargetKind, TargetSpec, Timing, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::TokenEntry;
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::GET_LOST,
    faces: &[Face {
        cost: ManaCost::new(1, [2, 0, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(
                    Types::CREATURE
                        .union(Types::ENCHANTMENT)
                        .union(Types::PLANESWALKER),
                ),
            )],
            body: destroy_and_map,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[&MAP],
};

/// The predefined Map token
/// ([CR 111.10s](https://yawgatog.com/resources/magic-rules/#R11110s)): "A Map token is a
/// colorless Map artifact token with "{1}, {T}, Sacrifice this token: Target creature you
/// control explores. Activate only as a sorcery.""
/// The Map token's name: its subtype plus the word "Token" (CR 111.4).
pub const MAP_TOKEN: &str = "Map Token";

pub static MAP: CardDef = CardDef {
    name: MAP_TOKEN,
    faces: &[Face {
        types: Types::ARTIFACT,
        subtypes: Subtypes::of(&[Subtype::Map]),
        abilities: &[Ability {
            kind: AbilityKind::Activated,
            cost: Cost {
                mana: ManaCost::generic(1),
                tap: true,
                sacrifice_self: true,
                ..Cost::NONE
            },
            timing: Timing::Sorcery,
            targets: &[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(Whose::You)]),
            )],
            may_pause: true,
            body: explore,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Its controller" is the permanent's controller as it was destroyed, read before it leaves
/// the battlefield ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an
/// effect requires information from the game ..., the answer is determined only once, when
/// the effect is applied. If the effect requires information from a specific object, ... the
/// effect uses the current information of that object if it's in the public zone it was
/// expected to be in; if it's no longer in that zone, ... the effect uses the object's last
/// known information."
fn destroy_and_map(ctx: &mut EffectCtx) -> Flow {
    let Some(permanent) = ctx.target_object(0) else {
        return Ok(());
    };
    let controller = ctx.state().controller(permanent.slot);
    ctx.destroy(permanent);
    ctx.create_tokens_for(controller, &MAP, 2, TokenEntry::default());
    Ok(())
}

/// Explore ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)).
fn explore(ctx: &mut EffectCtx) -> Flow {
    let Some(creature) = ctx.target_object(0) else {
        return Ok(());
    };
    ctx.explore(creature)
}

/// With no legal target it can't be cast
/// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's Mana Crypt is an
/// artifact, so passing is P0's only option.
pub const SCENARIOS: &[Scenario] = &[Scenario::new("Get Lost can't be cast without a target")
    .battlefield(
        P0,
        &[
            Permanent::new(names::PLAINS),
            Permanent::new(names::PLAINS),
            Permanent::new(names::PLAINS),
        ],
    )
    .hand(P0, &[names::GET_LOST])
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
