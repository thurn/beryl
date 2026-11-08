//! Subtlety. {2}{U}{U} Creature — Elemental Incarnation, 3/3. "Flash. Flying. When this
//! creature enters, choose up to one target creature spell or planeswalker spell. Its owner
//! puts it on their choice of the top or bottom of their library. Evoke—Exile a blue card
//! from your hand."

use beryl_engine::card::{
    Ability, CardDef, CastingOption, Castings, Colors, Face, FaceIx, Flow, Keywords, ManaCost,
    Subtype, Subtypes, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::Filter;
use beryl_engine::{Ask, DecisionKind, Seat, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// The spell leaves the stack without resolving, to the library of its owner, who chooses
/// the end ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object
/// would go to any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." The trigger resolves whether or not Subtlety is still there, as after
/// evoke's sacrifice ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)): "Once
/// activated or triggered, an ability exists on the stack independently of its source."
fn tuck<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(spell) = ctx.target_object(0) else {
        return Ok(());
    };
    let owner = ctx.state().object(spell.slot).owner;
    let top = ctx.choose_yes_no(owner, Ask::TopOrBottom { card: spell.slot })?;
    ctx.put_spell_into_library(spell, top);
    Ok(())
}

/// Evoke's cost exiles a blue card from the hand rather than paying {2}{U}{U}
/// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)), and its sacrifice
/// trigger enters alongside the tuck, both Subtlety's controller's to order
/// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b)).
pub static CARD: CardDef = CardDef {
    name: names::SUBTLETY,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 2, 0, 0, 0, 0]),
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Elemental, Subtype::Incarnation]),
        colors: Colors::BLUE,
        power: Some(3),
        toughness: Some(3),
        keywords: Keywords::FLASH.union(Keywords::FLYING),
        abilities: &[
            Ability {
                targets: &[TargetSpec::up_to_one(
                    TargetKind::Spell,
                    Filter::Types(Types::CREATURE.union(Types::PLANESWALKER)),
                )],
                may_pause: true,
                ..Ability::triggered(Trigger::Enters, bodies!(tuck))
            },
            Ability::evoke(),
        ],
        casting: &[CastingOption::Evoke {
            cost: ManaCost::NONE,
            exile: Colors::BLUE,
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

const SPELLBINDER: &str = names::ELITE_SPELLBINDER;
const TEFERI: &str = names::TEFERI_TIME_RAVELER;

const fn library(player: Seat, top: &'static [&'static str]) -> Action {
    Action::assert(Assertion::Library { player, top })
}

pub const SCENARIOS: &[Scenario] = &[
    // Cast for {2}{U}{U} in response to P1's Elite Spellbinder, which P1 puts on top. The
    // Plains in P1's hand keeps P1's main phase a decision once the stack is empty.
    Scenario::new("Subtlety hard cast: the creature spell's owner puts it on top")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P0, &[names::SUBTLETY])
        .battlefield(
            P1,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
            ],
        )
        .hand(P1, &[SPELLBINDER, names::PLAINS])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P1, SPELLBINDER),
            // Passing and the cast for mana: with no other blue card in hand, no evoke.
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 2,
            }),
            Action::cast(P0, names::SUBTLETY),
            Action::Resolve,
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            Action::Resolve,
            Action::answer(P1, DecisionKind::YesNo, Pick::Yes),
            library(P1, &[SPELLBINDER, names::ISLAND]),
            Action::assert(Assertion::Stack { count: 0 }),
            Action::assert(Assertion::Controls {
                player: P0,
                card: names::SUBTLETY,
                count: 1,
            }),
        ]),
    // Evoked with no mana, exiling Mana Drain, against P1's Teferi. P0 orders the triggers so
    // the sacrifice resolves first; the tuck resolves anyway, and P1 puts Teferi on the
    // bottom.
    Scenario::new("Subtlety evoked: sacrificed, its trigger still tucks a planeswalker spell")
        .at(2, P1, TurnStep::PrecombatMain)
        .hand(P0, &[names::SUBTLETY, names::MANA_DRAIN])
        .battlefield(
            P1,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P1, &[TEFERI, names::PLAINS])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P1, TEFERI),
            Action::Cast {
                player: P0,
                card: names::SUBTLETY,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: Some(names::MANA_DRAIN),
                count: 1,
            }),
            Action::Resolve,
            Action::choose(P0, DecisionKind::TriggerOrder, names::SUBTLETY),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            // The sacrifice: the tuck and Teferi are left.
            Action::Resolve,
            Action::assert(Assertion::Stack { count: 2 }),
            Action::Resolve,
            Action::answer(P1, DecisionKind::YesNo, Pick::No),
            library(P1, &[names::ISLAND, TEFERI]),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::SUBTLETY),
                count: 1,
            }),
        ]),
    // With mana for its cost and two other blue cards in hand, both casts are offered; evoked,
    // P0 picks which blue card to exile.
    Scenario::new("Subtlety with mana and blue cards: cast or evoke, then which card to exile")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(
            P0,
            &[names::SUBTLETY, names::TIME_SPIRAL, names::FRACTURED_IDENTITY],
        )
        .battlefield(
            P1,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
            ],
        )
        .hand(P1, &[SPELLBINDER, names::PLAINS])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P1, SPELLBINDER),
            // Passing, the cast for mana and the evoke.
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::Priority,
                candidates: 3,
            }),
            Action::Cast {
                player: P0,
                card: names::SUBTLETY,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Action::assert(Assertion::Decision {
                player: P0,
                kind: DecisionKind::ExileCost,
                candidates: 2,
            }),
            Action::choose(P0, DecisionKind::ExileCost, names::TIME_SPIRAL),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: Some(names::TIME_SPIRAL),
                count: 1,
            }),
            Action::Resolve,
            Action::choose(P0, DecisionKind::TriggerOrder, names::SUBTLETY),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            Action::Resolve,
            Action::Resolve,
            Action::answer(P1, DecisionKind::YesNo, Pick::Yes),
            library(P1, &[SPELLBINDER, names::ISLAND]),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::SUBTLETY),
                count: 1,
            }),
        ]),
];
