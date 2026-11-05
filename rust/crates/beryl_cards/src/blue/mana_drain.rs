//! Mana Drain. {U}{U} Instant. "Counter target spell. At the beginning of your next main phase,
//! add an amount of {C} equal to that spell's mana value."

use beryl_engine::card::ManaColor as Color;
use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Delayed, Face, Flow, ManaColor, ManaCost, TargetKind,
    TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::MANA_DRAIN,
    faces: &[Face {
        cost: ManaCost::new(0, [0, 2, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::BLUE,
        abilities: &[
            Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
                body: counter_and_delay,
                ..Ability::NONE
            },
            Ability::triggered(Trigger::Delayed, add_colorless),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// The mana value is read before the spell leaves the stack, X included
/// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
/// number chosen for it while the object is on the stack." The second sentence creates a
/// delayed triggered ability ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)):
/// "Delayed triggered abilities are created during the resolution of spells or abilities".
fn counter_and_delay(ctx: &mut EffectCtx) -> Flow {
    let Some(spell) = ctx.target_object(0) else {
        return Ok(());
    };
    let value = ctx.spell_mana_value(spell).unwrap_or(0);
    ctx.counter(spell, false);
    ctx.delay(1, Delayed::YourNextMainPhase, value);
    Ok(())
}

/// [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "When an effect instructs a
/// player to add mana, that mana goes into a player's mana pool."
fn add_colorless(ctx: &mut EffectCtx) -> Flow {
    let amount = ctx.trigger().amount;
    ctx.add_mana(ctx.controller(), ManaColor::Colorless, amount);
    Ok(())
}

/// P0 counters P1's Timetwister (mana value 3) in P1's precombat main phase. The mana waits
/// for P0's own next main phase: not P1's postcombat main, but P0's precombat main of turn 3,
/// where the delayed trigger goes on the stack as the phase begins.
///
/// Cast in P0's own precombat main phase, against the Mana Tithe P1 aims at P0's Mana Crypt,
/// the next main phase is the postcombat main of the same turn. The Plains in P0's hand
/// keeps P0's priority a decision there.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Mana Drain: the countered spell's mana value at the caster's next main phase")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
        )
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS])
        .battlefield(
            P1,
            &[
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P1, &[names::TIMETWISTER])
        .actions(&[
            Action::cast(P1, names::TIMETWISTER),
            Action::cast(P0, names::MANA_DRAIN),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::TIMETWISTER),
                count: 1,
            }),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PostcombatMain,
            },
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 0,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::pass(P0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 3,
            }),
        ]),
    Scenario::new("Mana Drain on the caster's own turn: mana at that turn's postcombat main")
        .battlefield(
            P0,
            &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
        )
        .hand(P0, &[names::MANA_CRYPT, names::MANA_DRAIN, names::PLAINS])
        .battlefield(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::MANA_TITHE])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::MANA_CRYPT),
            Action::pass(P0),
            Action::cast(P1, names::MANA_TITHE),
            Action::cast(P0, names::MANA_DRAIN),
            Action::answer(
                P0,
                DecisionKind::Target,
                Pick::Card {
                    card: names::MANA_TITHE,
                },
            ),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::MANA_TITHE),
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Battlefield,
                card: Some(names::MANA_CRYPT),
                count: 1,
            }),
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 0,
            }),
            Action::AdvanceTo {
                turn: 1,
                step: TurnStep::PostcombatMain,
            },
            Action::pass(P0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 1,
            }),
        ]),
];
