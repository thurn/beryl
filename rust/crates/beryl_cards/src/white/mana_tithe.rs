//! Mana Tithe. {W} Instant. "Counter target spell unless its controller pays {1}."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Castings, Colors, Face, FaceIx, Flow, ManaCost, TargetKind,
    TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::MANA_TITHE,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            may_pause: true,
            body: counter_unless_pays_one,
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// [CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a): "[Do something] unless
/// [a player does something else]" means "[A player may do something else]. If [that player
/// doesn't], [do something]." A controller who can't pay is never asked.
fn counter_unless_pays_one(ctx: &mut EffectCtx) -> Flow {
    let Some(spell) = ctx.target_object(0) else {
        return Ok(());
    };
    let Some(controller) = ctx.spell_controller(spell) else {
        return Ok(());
    };
    if !ctx.unless_pays(controller, ManaCost::generic(1))? {
        ctx.counter(spell, false);
    }
    Ok(())
}

/// Four lands for an impending Overlord, two to pay with.
const SIX_LANDS: &[Permanent] = &[
    Permanent::new(names::ISLAND),
    Permanent::new(names::SPARAS_HEADQUARTERS),
    Permanent::new(names::SEACHROME_COAST),
    Permanent::new(names::PLAINS),
    Permanent::new(names::ISLAND),
    Permanent::new(names::PLAINS),
];

/// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. Mana
/// Crypt is the spell to counter: it costs nothing, so its controller's lands stay free
/// to pay the tithe. Strip Mine's activation keeps P0's priority a decision.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Mana Tithe: the controller pays {1}")
        .battlefield(P0, &[Permanent::new(names::STRIP_MINE)])
        .hand(P0, &[names::MANA_CRYPT])
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::MANA_TITHE])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::pass(P0),
            Action::cast(P1, names::MANA_TITHE),
            Action::pass(P0),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Battlefield,
                card: Some(names::MANA_CRYPT),
                count: 1,
            }),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::STRIP_MINE,
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::MANA_TITHE),
                count: 1,
            }),
        ]),
    // Overlord cast for impending ({2}{W}{W}) taps four of P0's six lands, and the two left
    // pay the tithe. (A game once showed it countered unasked from six lands: Elite
    // Spellbinder's {2} tax had tapped them all.)
    Scenario::new("Mana Tithe: lands left after paying for the spell pay {1}")
        .battlefield(P0, SIX_LANDS)
        .hand(P0, &[names::OVERLORD_OF_THE_MISTMOORS])
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::MANA_TITHE])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::Cast {
                player: P0,
                card: names::OVERLORD_OF_THE_MISTMOORS,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Action::cast(P1, names::MANA_TITHE),
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::End,
            },
            Action::assert(Assertion::Controls {
                player: P0,
                card: names::OVERLORD_OF_THE_MISTMOORS,
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::MANA_TITHE),
                count: 1,
            }),
        ]),
    Scenario::new("Mana Tithe: a controller who can't pay isn't asked")
        .hand(P0, &[names::MANA_CRYPT])
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::MANA_TITHE])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::cast(P1, names::MANA_TITHE),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::MANA_CRYPT),
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Battlefield,
                card: None,
                count: 0,
            }),
        ]),
];
