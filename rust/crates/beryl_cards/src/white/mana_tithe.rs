//! Mana Tithe. {W} Instant. "Counter target spell unless its controller pays {1}."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::state::TargetPlace;
use beryl_engine::{Layout, bodies};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::MANA_TITHE,
    faces: &[Face {
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::WHITE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any).counter()],
            may_pause: true,
            body: bodies!(counter_unless_pays_one),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// [CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a): "[Do something] unless
/// [a player does something else]" means "[A player may do something else]. If [that player
/// doesn't], [do something]." A controller who can't pay is never asked.
fn counter_unless_pays_one<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(spell) = ctx.target_object(TargetPlace::FIRST) else {
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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    /// Four lands for the planeswalker, two to pay with.
    const SIX_LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(props::NONBASIC_LAND),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::ISLAND),
        Permanent::new(names::PLAINS),
    ];

    /// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. The
    /// artifact is the spell to counter: it costs {1}, so one of its controller's two lands
    /// stays free to pay the tithe.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Mana Tithe: the controller pays {1}")
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[props::ARTIFACT])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::cast(P1, names::MANA_TITHE),
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 2,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_TITHE),
                    count: 1,
                }),
            ]),
        // The planeswalker ({2}{U}{U}) taps four of P0's six lands, and the two left pay the
        // tithe. (A game once showed it countered unasked from six lands: Elite Spellbinder's
        // {2} tax had tapped them all.)
        Scenario::new("Mana Tithe: lands left after paying for the spell pay {1}")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[props::PLANESWALKER])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::PLANESWALKER),
                Action::cast(P1, names::MANA_TITHE),
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::PLANESWALKER,
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
            .battlefield(P0, &[Permanent::new(names::PLAINS)])
            .hand(P0, &[props::ARTIFACT])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::cast(P1, names::MANA_TITHE),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(props::ARTIFACT),
                    count: 0,
                }),
            ]),
    ];
}
