//! Damn. {B}{B} Sorcery. "Destroy target creature. A creature destroyed this way can't be
//! regenerated. Overload {2}{W}{W} (You may cast this spell for its overload cost. If you do,
//! change "target" in its text to "each.")"

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, CastingOption, Castings, Colors, Face, FaceIx, Flow, ManaCost,
    TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

const CREATURES: Filter = Filter::Types(Types::CREATURE);

/// "Can't be regenerated" ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)):
/// such effects "cause regeneration shields to not be applied." No card in the cube
/// regenerates, so destroying is all it takes.
fn destroy_target(ctx: &mut EffectCtx) -> Flow {
    if let Some(creature) = ctx.target_object(0) {
        ctx.destroy(creature);
    }
    Ok(())
}

/// Overloaded, "Destroy each creature" ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)).
/// It targets nothing, so hexproof and protection don't stop it
/// ([CR 702.96b](https://yawgatog.com/resources/magic-rules/#R70296b)): "If a player
/// chooses to pay the overload cost of a spell, that spell won't require any targets."
/// The creatures are destroyed at once.
fn destroy_each(ctx: &mut EffectCtx) -> Flow {
    let s = ctx.state();
    let creatures = CREATURES.select(
        s,
        ctx.table(),
        s.battlefield().iter().copied().collect(),
        ctx.controller(),
    );
    ctx.destroy_all(creatures);
    Ok(())
}

/// Its color is its mana cost's, black: the overload cost doesn't change it
/// ([CR 202.2](https://yawgatog.com/resources/magic-rules/#R2022)): "An object is the color or
/// colors of the mana symbols in its mana cost". The cube lists it with white for its
/// overload.
pub static CARD: CardDef = CardDef {
    name: names::DAMN,
    faces: &[Face {
        cost: ManaCost::new(0, [0, 0, 2, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::BLACK,
        abilities: &[
            Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(TargetKind::Permanent, CREATURES)],
                body: destroy_target,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Spell,
                body: destroy_each,
                ..Ability::NONE
            },
        ],
        casting: &[CastingOption::Overload {
            cost: ManaCost::new(2, [2, 0, 0, 0, 0, 0]),
            ability: 1,
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const SPELLBINDER: &str = names::ELITE_SPELLBINDER;
const ADELINE: &str = names::ADELINE_RESPLENDENT_CATHAR;

const fn creatures(player: beryl_engine::Seat, card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::Controls {
        player,
        card,
        count,
    })
}

pub const SCENARIOS: &[Scenario] = &[
    // For {B}{B}, one target: P0 destroys P1's Elite Spellbinder and keeps Adeline.
    Scenario::new("Damn for {B}{B} destroys its target alone")
        .battlefield(
            P0,
            &[
                Permanent::new(names::SWAMP),
                Permanent::new(names::SWAMP),
                Permanent::new(ADELINE),
            ],
        )
        .hand(P0, &[names::DAMN])
        .battlefield(P1, &[Permanent::new(SPELLBINDER)])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::DAMN),
            Action::choose(P0, DecisionKind::Target, SPELLBINDER),
            Action::Resolve,
            creatures(P1, SPELLBINDER, 0),
            creatures(P0, ADELINE, 1),
        ]),
    // With only white mana, the overload is the one way to cast it: two candidates, passing
    // and the overloaded cast. It asks for no target and destroys every creature, P0's own
    // among them.
    Scenario::new("Damn overloaded destroys each creature, targeting none")
        .battlefield(
            P0,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(ADELINE),
            ],
        )
        .hand(P0, &[names::DAMN])
        .battlefield(
            P1,
            &[
                Permanent::new(SPELLBINDER),
                Permanent::new(names::THIEVING_SKYDIVER),
            ],
        )
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 2,
            }),
            Action::Cast {
                player: P0,
                card: names::DAMN,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Action::Resolve,
            creatures(P0, ADELINE, 0),
            creatures(P1, SPELLBINDER, 0),
            creatures(P1, names::THIEVING_SKYDIVER, 0),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: None,
                count: 2,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: None,
                count: 2,
            }),
        ]),
];
