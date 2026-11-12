//! Elite Spellbinder. {2}{W} Creature — Human Cleric, 3/1. "Flying. When this creature
//! enters, look at target opponent's hand. You may exile a nonland card from it. For as long
//! as that card remains exiled, its owner may play it. A spell cast this way costs {2} more
//! to cast."

use beryl_engine::card::{
    Ability, CardDef, Colors, Face, Flow, Keywords, ManaCost, Place, Prompt, Purpose, Subtype,
    Subtypes, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::filter::{Evaluate, Filter, Whose};
use beryl_engine::state::TargetPlace;
use beryl_engine::{Layout, bodies};
use beryl_engine::{Zone, ZoneKey};

use crate::names;

/// Looking at a hand shows it to the looker alone; the exiled card is public. Its owner may
/// cast it from exile for as long as it stays there
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)), and the {2} is a cost
/// increase ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)): the total cost
/// is the mana cost "plus all additional costs and cost increases". The card is nonland, so
/// playing it is casting it.
fn spellbind<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(them) = ctx.target_player(TargetPlace::FIRST) else {
        return Ok(());
    };
    let you = ctx.controller();
    let hand = ctx.state().zone(ZoneKey::Of(them, Zone::Hand));
    ctx.look_at(you, hand);
    let nonland =
        Filter::Not(&Filter::Types(Types::LAND)).select(ctx.state(), ctx.table(), hand, you);
    // The whole hand is looked at, for one card to exile.
    let look = Prompt::new(Purpose::Look)
        .most(1)
        .places(Place::Exile, Place::None);
    if let Some(slot) = ctx.may_choose_object(you, nonland, look)? {
        ctx.move_to(ctx.state().object_ref(slot), Zone::Exile, Reason::Exile);
        let exiled = ctx.state().object_ref(slot);
        ctx.allow_cast(exiled, them, 2, false, false);
    }
    Ok(())
}

pub static CARD: CardDef = CardDef {
    name: names::ELITE_SPELLBINDER,
    faces: &[Face {
        cost: ManaCost::new(2, [1, 0, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Human, Subtype::Cleric]),
        colors: Colors::WHITE,
        power: Some(3),
        toughness: Some(1),
        keywords: Keywords::FLYING,
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(Whose::AnOpponent),
            )],
            may_pause: true,
            ..Ability::triggered(&Trigger::ENTERS, bodies!(spellbind))
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    const SKYDIVER: &str = names::THIEVING_SKYDIVER;

    const THREE_PLAINS: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // The exiled Skydiver costs its owner {3}{U} on their turn: all four Islands.
        Scenario::new("Spellbinder's card costs its owner {2} more")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, THREE_PLAINS)
            .hand(P1, &[SKYDIVER, names::ISLAND])
            .battlefield(
                P1,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SPELLBINDER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                // The enters trigger, which targets the one opponent, resolves up to its question.
                Action::Assert {
                    assertion: Assertion::Stack { count: 1 },
                },
                Action::Resolve,
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::YesNo,
                    pick: Pick::Yes,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Exile,
                        card: Some(SKYDIVER),
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Hand,
                        card: Some(SKYDIVER),
                        count: 0,
                    },
                },
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                // Pass, an Island to play, the one P0 saw or the one just drawn, and the Skydiver
                // unkicked: kicked, it would need five.
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P1,
                        kind: DecisionKind::Priority,
                        candidates: 4,
                    },
                },
                Action::Cast {
                    player: P1,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P1,
                        card: names::ISLAND,
                        count: 4,
                    },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P1,
                        card: SKYDIVER,
                        count: 1,
                    },
                },
            ]),
        // Only lands: nothing to exile, nothing to ask.
        Scenario::new("Spellbinder sees a hand of lands")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, THREE_PLAINS)
            .hand(P1, &[names::ISLAND, names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SPELLBINDER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::PostcombatMain,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Exile,
                        card: None,
                        count: 0,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Hand,
                        card: None,
                        count: 2,
                    },
                },
            ]),
    ];
}
