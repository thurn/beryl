//! Aang's Iceberg. {2}{W} Enchantment.
//!
//! "Flash
//! When this enchantment enters, exile up to one other target nonland permanent until this
//! enchantment leaves the battlefield.
//! Waterbend {3}: Sacrifice this enchantment. If you do, scry 2. (While paying a waterbend
//! cost, you can tap your artifacts and creatures to help. Each one pays for {1}.)"

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Cost, Face, Flow, Keywords, ManaCost, TargetKind,
    TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::state::{TargetPlace, Zone};
use beryl_engine::{Layout, bodies};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::AANGS_ICEBERG,
    faces: &[Face {
        cost: ManaCost::new(2, [1, 0, 0, 0, 0, 0]),
        types: Types::ENCHANTMENT,
        colors: Colors::WHITE,
        keywords: Keywords::FLASH,
        abilities: &[
            Ability {
                targets: &[TargetSpec::up_to_one(
                    TargetKind::Permanent,
                    Filter::Not(&Filter::Types(Types::LAND)),
                )
                .other()
                .removal()],
                ..Ability::triggered(&Trigger::ENTERS, bodies!(exile_until_this_leaves))
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost {
                    mana: ManaCost::generic(3),
                    waterbend: 3,
                    ..Cost::NONE
                },
                may_pause: true,
                body: bodies!(sacrifice_and_scry),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Exile up to one other target nonland permanent until this enchantment leaves the
/// battlefield" ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)). If the
/// Iceberg has already left by the time this resolves, the permanent isn't exiled
/// ([CR 610.3b](https://yawgatog.com/resources/magic-rules/#R6103b)).
fn exile_until_this_leaves<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(permanent) = ctx.target_object(TargetPlace::FIRST) {
        ctx.exile_until_source_leaves(permanent);
    }
    Ok(())
}

/// "Sacrifice this enchantment. If you do, scry 2." Its controller can sacrifice it only if
/// it is still on the battlefield ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)).
fn sacrifice_and_scry<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let iceberg = ctx.source();
    if !ctx.state().is_current(iceberg)
        || ctx.state().object(iceberg.slot).zone() != Zone::Battlefield
    {
        return Ok(());
    }
    ctx.sacrifice(iceberg);
    ctx.scry(ctx.controller(), 2)
}

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::state::Zone;
    use beryl_engine::{DecisionKind, TurnStep};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;
    use crate::tokens::TREASURE_TOKEN;

    const ICEBERG: &str = names::AANGS_ICEBERG;

    const STAFF: &str = names::STAFF_OF_THE_STORYTELLER;

    const JACE: &str = names::JACE_VRYNS_PRODIGY;

    const PLAINS: &str = names::PLAINS;

    const ISLAND: &str = names::ISLAND;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("exiles a permanent until it leaves; waterbend taps a creature to pay")
            .at(2, P1, TurnStep::End)
            .hand(P0, &[ICEBERG])
            .battlefield(
                P0,
                &[
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                    Permanent::new(PLAINS),
                    Permanent::new(JACE),
                ],
            )
            .battlefield(P1, &[Permanent::new(STAFF)])
            .library(P0, &[ISLAND, STAFF, PLAINS, ISLAND])
            .library(P1, &[PLAINS])
            .actions(&[
                // Flash: at the end of the opponent's turn, exiling their artifact rather than
                // the Iceberg's own controller's Jace.
                Action::cast(P0, ICEBERG),
                Action::Resolve,
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::choose(P0, DecisionKind::Target, STAFF),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(STAFF),
                    count: 1,
                }),
                // Its controller's turn: three Plains untapped and a creature, for waterbend {3}.
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, ICEBERG, 1),
                // CR 701.67a: Jace taps for one generic mana; two Plains pay the rest.
                Action::answer(P0, DecisionKind::ManaPayment, Pick::Card { card: JACE }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: PLAINS,
                    count: 2,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: JACE,
                    count: 1,
                }),
                // Scry 2 over Staff and Plains: Staff to the bottom, Plains stays on top.
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Card { card: STAFF }),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[PLAINS, ISLAND, STAFF],
                }),
                // Sacrificed, so the Staff returns under its owner's control (CR 610.3c): a new
                // object, whose enters ability makes a Spirit.
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(ICEBERG),
                    count: 1,
                }),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: STAFF,
                    count: 1,
                }),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: super::super::staff_of_the_storyteller::SPIRIT_TOKEN,
                    count: 1,
                }),
            ]),
        Scenario::new("waterbend taps a creature and an artifact; scry reorders the top")
            .battlefield(
                P0,
                &[
                    Permanent::new(ICEBERG),
                    Permanent::new(PLAINS),
                    Permanent::new(JACE),
                    Permanent::new(STAFF),
                ],
            )
            .library(P0, &[ISLAND, PLAINS])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::activate(P0, ICEBERG, 1),
                // One Plains can't pay {3} alone: no paying the rest with mana yet.
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ManaPayment,
                    candidates: 2,
                }),
                Action::answer(P0, DecisionKind::ManaPayment, Pick::Card { card: STAFF }),
                // Then Jace is the only way left to pay one more, and the Plains pays the last.
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: JACE,
                    count: 1,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: PLAINS,
                    count: 1,
                }),
                // Nothing to the bottom; the Plains on top of the Island.
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::choose(P0, DecisionKind::ChooseObject, PLAINS),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[PLAINS, ISLAND],
                }),
            ]),
        Scenario::new("waterbend uses mana sources without sacrificing them")
            .battlefield(
                P0,
                &[
                    Permanent::new(ICEBERG),
                    Permanent::new(PLAINS),
                    Permanent::new(names::MANA_CRYPT),
                    Permanent::new(JACE),
                    Permanent::new(names::CURRENCY_CONVERTER),
                ],
            )
            .hand(P0, &[PLAINS])
            .library(P0, &[ISLAND, PLAINS, ISLAND])
            .library(P1, &[PLAINS])
            .actions(&[
                // Jace and the Converter create the Treasure while tapping themselves.
                Action::activate(P0, JACE, 0),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, PLAINS),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::activate(P0, names::CURRENCY_CONVERTER, 2),
                Action::Resolve,
                Action::activate(P0, ICEBERG, 1),
                // A Treasure tapped to waterbend does not activate its own mana ability or
                // sacrifice itself (CR 701.67a and CR 111.10a).
                Action::choose(P0, DecisionKind::ManaPayment, TREASURE_TOKEN),
                Action::choose(P0, DecisionKind::ManaPayment, names::MANA_CRYPT),
                Action::choose(P0, DecisionKind::ChooseUpTo, ISLAND),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: TREASURE_TOKEN,
                    count: 1,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::MANA_CRYPT,
                    count: 1,
                }),
            ]),
    ];
}
