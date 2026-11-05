//! Jace, Vryn's Prodigy // Jace, Telepath Unbound. A transforming double-faced card.
//!
//! Front: {1}{U} Legendary Creature — Human Wizard, 0/2. "{T}: Draw a card, then discard a
//! card. If there are five or more cards in your graveyard, exile Jace, then return him to the
//! battlefield transformed under his owner's control."
//!
//! Back: Legendary Planeswalker — Jace. Loyalty 5. "+1: Up to one target creature gets -2/-0
//! until your next turn.
//! −3: You may cast target instant or sorcery card from your graveyard this turn. If that
//! spell would be put into your graveyard, exile it instead.
//! −9: You get an emblem with 'Whenever you cast a spell, target opponent mills five cards.'"

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Cost, Face, Flow, ManaCost, Modification, Subtype,
    Subtypes, Supertypes, TargetKind, TargetSpec, Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::state::{Continuous, CounterKind, Duration};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// [CR 712.2](https://yawgatog.com/resources/magic-rules/#R7122): a double-faced card "has a
/// Magic card face on each side rather than a Magic card face on one side and a Magic card
/// back on the other."
pub static CARD: CardDef = CardDef {
    name: names::JACE_VRYNS_PRODIGY,
    faces: &[
        Face {
            cost: ManaCost::new(1, [0, 1, 0, 0, 0, 0]),
            supertypes: Supertypes::LEGENDARY,
            types: Types::CREATURE,
            subtypes: Subtypes::of(&[Subtype::Human, Subtype::Wizard]),
            colors: Colors::BLUE,
            power: Some(0),
            toughness: Some(2),
            abilities: &[Ability {
                kind: AbilityKind::Activated,
                cost: Cost::TAP,
                may_pause: true,
                body: loot_and_transform,
                ..Ability::NONE
            }],
            ..Face::EMPTY
        },
        Face {
            name: TELEPATH,
            supertypes: Supertypes::LEGENDARY,
            types: Types::PLANESWALKER,
            subtypes: Subtypes::of(&[Subtype::Jace]),
            // CR 712.8e: the back face has only its own characteristics. Its color indicator
            // makes it blue (CR 204.1).
            colors: Colors::BLUE,
            loyalty: Some(5),
            abilities: &[
                Ability {
                    kind: AbilityKind::Activated,
                    cost: Cost::loyalty(1),
                    targets: &[TargetSpec::up_to_one(
                        TargetKind::Permanent,
                        Filter::Types(Types::CREATURE),
                    )],
                    body: weaken,
                    ..Ability::NONE
                },
                Ability {
                    kind: AbilityKind::Activated,
                    cost: Cost::loyalty(-3),
                    targets: &[TargetSpec::one(
                        TargetKind::GraveyardCard,
                        Filter::All(&[
                            Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                            Filter::Whose(Whose::You),
                        ]),
                    )],
                    body: recast,
                    ..Ability::NONE
                },
                Ability {
                    kind: AbilityKind::Activated,
                    cost: Cost::loyalty(-9),
                    body: emblem,
                    ..Ability::NONE
                },
            ],
            transformed: true,
            ..Face::EMPTY
        },
    ],
    tokens: &[&EMBLEM],
};

/// The emblem of the −9 ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An
/// emblem is a marker used to represent an object that has one or more abilities, but usually
/// no other characteristics."
pub static EMBLEM: CardDef = CardDef {
    name: "Jace, Telepath Unbound Emblem",
    faces: &[Face {
        abilities: &[Ability {
            targets: &[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(Whose::Opponent),
            )],
            ..Ability::triggered(Trigger::YouCast, mill_five)
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}: Draw a card, then discard a card. If there are five or more cards in your graveyard,
/// exile Jace, then return him to the battlefield transformed under his owner's control."
/// The graveyard is counted after the discard, as the instructions are followed in order
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)).
fn loot_and_transform(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    ctx.draw(you, 1);
    ctx.discard_a_card(you)?;
    if ctx.state().graveyard(you).len() >= 5 {
        ctx.exile_and_return_transformed(ctx.source());
    }
    Ok(())
}

/// "+1: Up to one target creature gets -2/-0 until your next turn." A layer 7c effect
/// ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
/// that modify power and/or toughness (but don't set power and/or toughness to a specific
/// number or value) are applied." It lasts "as long as stated by the spell or ability creating
/// it" ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)).
fn weaken(ctx: &mut EffectCtx) -> Flow {
    if let Some(creature) = ctx.target_object(0) {
        ctx.add_effect(
            creature,
            Continuous::Layer(Modification::ModifyPt(-2, 0)),
            Duration::UntilYourNextTurn(ctx.controller()),
        );
    }
    Ok(())
}

/// "−3: You may cast target instant or sorcery card from your graveyard this turn. If that
/// spell would be put into your graveyard, exile it instead." The permission carries the
/// replacement to the spell it casts: a new object on the stack
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which "exile it instead"
/// follows wherever it would go to the graveyard: resolving, countered or fizzling
/// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)).
fn recast(ctx: &mut EffectCtx) -> Flow {
    if let Some(card) = ctx.target_object(0) {
        ctx.allow_cast(card, ctx.controller(), 0, true, true);
    }
    Ok(())
}

/// "−9: You get an emblem with 'Whenever you cast a spell, target opponent mills five
/// cards.'" [CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142): "This means that
/// [player] puts an emblem with [ability] into the command zone. The emblem is both owned and
/// controlled by that player."
fn emblem(ctx: &mut EffectCtx) -> Flow {
    ctx.create_emblem(ctx.controller(), &EMBLEM);
    Ok(())
}

/// "Whenever you cast a spell, target opponent mills five cards."
/// [CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a): "For a player to mill a
/// number of cards, that player puts that many cards from the top of their library into their
/// graveyard."
fn mill_five(ctx: &mut EffectCtx) -> Flow {
    if let Some(player) = ctx.target_player(0) {
        ctx.mill(player, 5);
    }
    Ok(())
}

const JACE: &str = names::JACE_VRYNS_PRODIGY;
/// The back face's name, which the transformed permanent has (CR 712.8e).
const TELEPATH: &str = "Jace, Telepath Unbound";
const ISLAND: &str = names::ISLAND;
const PLAINS: &str = names::PLAINS;
const STAFF: &str = names::STAFF_OF_THE_STORYTELLER;

const fn loyalty(count: u8) -> Action {
    Action::assert(Assertion::Counters {
        player: P0,
        card: TELEPATH,
        kind: CounterKind::Loyalty,
        count,
    })
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("looting to five cards in the graveyard transforms him")
        .battlefield(P0, &[Permanent::new(JACE)])
        .hand(P0, &[PLAINS])
        .graveyard(P0, &[ISLAND, ISLAND, ISLAND])
        .library(P0, &[ISLAND, ISLAND, ISLAND])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            // Four in the graveyard after the first loot: no transformation.
            Action::activate(P0, JACE, 0),
            Action::Resolve,
            Action::choose(P0, DecisionKind::Discard, PLAINS),
            Action::assert(Assertion::Creature {
                player: P0,
                card: JACE,
                creature: true,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            // Nothing else to do: the loot resolves as soon as its controller passes, and
            // with only Islands in hand, the discard needs no choice.
            Action::activate(P0, JACE, 0),
            // Exiled and returned with his back face up: a planeswalker with loyalty 5.
            Action::assert(Assertion::Creature {
                player: P0,
                card: TELEPATH,
                creature: false,
            }),
            loyalty(5),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: None,
                count: 5,
            }),
            // A new object, so its loyalty abilities may be activated this turn. No creature
            // to target: none is chosen.
            Action::activate(P0, TELEPATH, 0),
            loyalty(6),
        ]),
    Scenario::new("+1 shrinks a creature until his controller's next turn")
        .battlefield(P0, &[Permanent::new(JACE).transformed()])
        .battlefield(P1, &[Permanent::new(JACE)])
        .library(P0, &[ISLAND, ISLAND])
        .library(P1, &[PLAINS, PLAINS])
        .actions(&[
            Action::activate(P0, TELEPATH, 0),
            Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
            Action::Resolve,
            loyalty(6),
            Action::assert(Assertion::Pt {
                player: P1,
                card: JACE,
                token: false,
                power: -2,
                toughness: 2,
            }),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P1),
            Action::answer(P1, DecisionKind::Attacker, Pick::Done),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::End,
            },
            Action::assert(Assertion::Pt {
                player: P1,
                card: JACE,
                token: false,
                power: -2,
                toughness: 2,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::Upkeep,
            },
            Action::assert(Assertion::Pt {
                player: P1,
                card: JACE,
                token: false,
                power: 0,
                toughness: 2,
            }),
        ]),
    Scenario::new("−3 recasts a sorcery from the graveyard, which is then exiled")
        .battlefield(
            P0,
            &[Permanent::new(JACE).transformed(), Permanent::new(PLAINS)],
        )
        .graveyard(P0, &[names::OUST])
        // A land to play keeps the game in this main phase for the assertions.
        .hand(P0, &[ISLAND])
        .battlefield(P1, &[Permanent::new(JACE)])
        .library(P0, &[ISLAND])
        .library(P1, &[PLAINS, ISLAND])
        .actions(&[
            Action::activate(P0, TELEPATH, 1),
            Action::Resolve,
            loyalty(2),
            Action::cast(P0, names::OUST),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Exile,
                card: Some(names::OUST),
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: None,
                count: 0,
            }),
            Action::assert(Assertion::Library {
                player: P1,
                top: &[PLAINS, JACE, ISLAND],
            }),
        ]),
    Scenario::new("−9's emblem mills an opponent five whenever its owner casts a spell")
        .battlefield(
            P0,
            &[
                Permanent::new(JACE)
                    .transformed()
                    .counters(CounterKind::Loyalty, 9),
                Permanent::new(PLAINS),
                Permanent::new(PLAINS),
            ],
        )
        .hand(P0, &[STAFF, PLAINS])
        .library(P0, &[ISLAND])
        .library(P1, &[PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, ISLAND])
        .actions(&[
            Action::activate(P0, TELEPATH, 2),
            // Loyalty 0: he is put into his owner's graveyard; the emblem stays.
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(JACE),
                count: 1,
            }),
            // The trigger goes on the stack above the Staff and resolves first.
            Action::cast(P0, STAFF),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(PLAINS),
                count: 5,
            }),
            Action::assert(Assertion::Library {
                player: P1,
                top: &[ISLAND],
            }),
            Action::assert(Assertion::Controls {
                player: P0,
                card: STAFF,
                count: 1,
            }),
        ]),
];
