//! Jace, the Mind Sculptor. {2}{U}{U} Legendary Planeswalker — Jace. Loyalty 3.
//!
//! "+2: Look at the top card of target player's library. You may put that card on the bottom
//! of that player's library.
//! 0: Draw three cards, then put two cards from your hand on top of your library in any order.
//! −1: Return target creature to its owner's hand.
//! −12: Exile all cards from target player's library, then that player shuffles their hand
//! into their library."

use beryl_engine::Bitboard;
use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Cost, Face, Flow, ManaCost, Subtype, Subtypes,
    Supertypes, TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Reason;
use beryl_engine::filter::Filter;
use beryl_engine::state::CounterKind;
use beryl_engine::{Ask, DecisionKind, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

const TARGET_PLAYER: &[TargetSpec] = &[TargetSpec::one(TargetKind::Player, Filter::Any)];

pub static CARD: CardDef = CardDef {
    name: names::JACE_THE_MIND_SCULPTOR,
    faces: &[Face {
        cost: ManaCost::new(2, [0, 2, 0, 0, 0, 0]),
        supertypes: Supertypes::LEGENDARY,
        types: Types::PLANESWALKER,
        subtypes: Subtypes::of(&[Subtype::Jace]),
        colors: Colors::BLUE,
        loyalty: Some(3),
        abilities: &[
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(2),
                targets: TARGET_PLAYER,
                may_pause: true,
                body: bodies!(fateseal),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(0),
                may_pause: true,
                body: bodies!(brainstorm),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-1),
                targets: &[
                    TargetSpec::one(TargetKind::Permanent, Filter::Types(Types::CREATURE))
                        .removal(),
                ],
                body: bodies!(bounce),
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(-12),
                targets: TARGET_PLAYER,
                body: bodies!(exile_library),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "+2: Look at the top card of target player's library. You may put that card on the
/// bottom of that player's library." Looking shows the card to Jace's controller alone, an
/// exception to [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't
/// look at or change the order of cards in a library." Moving it within the library is no
/// zone change, so it stays the same object.
fn fateseal<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(player) = ctx.target_player(0) else {
        return Ok(());
    };
    let Some(top) = ctx.top_of_library(player, 1).first() else {
        return Ok(());
    };
    let top = ctx.state().object_ref(top);
    let you = ctx.controller();
    ctx.look_at(you, Bitboard::single(top.slot));
    if ctx.choose_yes_no(you, Ask::Fateseal { player })? {
        ctx.put_library_card_on_bottom(top);
    }
    Ok(())
}

/// "0: Draw three cards, then put two cards from your hand on top of your library in any
/// order." Each card chosen goes on top of the one before, so the second is the top card.
/// [CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212): "If a player is instructed to
/// draw multiple cards, that player performs that many individual card draws."
fn brainstorm<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let you = ctx.controller();
    ctx.draw(you, 3);
    for count in (1..=2).rev() {
        let hand = ctx.state().zone(you, Zone::Hand);
        if hand.is_empty() {
            break;
        }
        let card = ctx.choose_object(you, hand, Ask::PutBack { count })?;
        let card = ctx.state().object_ref(card);
        ctx.move_to(card, Zone::Library, Reason::Effect);
    }
    Ok(())
}

/// "−1: Return target creature to its owner's hand."
/// [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone."
fn bounce<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(creature) = ctx.target_object(0) {
        ctx.return_to_hand(creature);
    }
    Ok(())
}

/// "−12: Exile all cards from target player's library, then that player shuffles their hand
/// into their library." [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a):
/// "To exile an object, move it to the exile zone from wherever it is." Then
/// [CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a library
/// ... randomize the cards within it so that no player knows their order."
fn exile_library<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(player) = ctx.target_player(0) else {
        return Ok(());
    };
    let library = ctx.state().zone(player, Zone::Library);
    ctx.move_all(library, Zone::Exile, Reason::Exile);
    let hand = ctx.state().zone(player, Zone::Hand);
    ctx.move_all(hand, Zone::Library, Reason::Effect);
    ctx.shuffle(player);
    Ok(())
}

const JACE: &str = names::JACE_THE_MIND_SCULPTOR;
const ISLAND: &str = names::ISLAND;
const PLAINS: &str = names::PLAINS;
const STAFF: &str = names::STAFF_OF_THE_STORYTELLER;
const PRODIGY: &str = names::JACE_VRYNS_PRODIGY;

const fn loyalty(count: u8) -> Action {
    Action::assert(Assertion::Counters {
        player: P0,
        card: JACE,
        kind: CounterKind::Loyalty,
        count,
    })
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("+2 puts the top of an opponent's library on the bottom")
        .battlefield(P0, &[Permanent::new(JACE)])
        .library(P0, &[ISLAND, PLAINS])
        .library(P1, &[STAFF, PLAINS, ISLAND])
        .actions(&[
            Action::activate(P0, JACE, 0),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
            Action::Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
            // Assertions read the next open decision: by then, the opponent has drawn the
            // Plains that the Staff left on top.
            Action::assert(Assertion::Library {
                player: P1,
                top: &[ISLAND, STAFF],
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Hand,
                card: Some(PLAINS),
                count: 1,
            }),
            loyalty(5),
            // The next turn, Jace looks at his own top card and keeps it there.
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::activate(P0, JACE, 0),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P0 }),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            Action::assert(Assertion::Library {
                player: P0,
                top: &[PLAINS],
            }),
            loyalty(7),
        ]),
    Scenario::new("0 draws three and puts two back in the order chosen")
        .battlefield(P0, &[Permanent::new(JACE)])
        .hand(P0, &[STAFF])
        .library(P0, &[ISLAND, PLAINS, PRODIGY, ISLAND])
        .library(P1, &[PLAINS])
        .actions(&[
            Action::activate(P0, JACE, 1),
            Action::Resolve,
            Action::choose(P0, DecisionKind::ChooseObject, STAFF),
            Action::choose(P0, DecisionKind::ChooseObject, ISLAND),
            Action::assert(Assertion::Library {
                player: P0,
                top: &[ISLAND, STAFF, ISLAND],
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Hand,
                card: None,
                count: 2,
            }),
            loyalty(3),
        ]),
    Scenario::new("−1 returns a creature; −12 exiles a library and shuffles in the hand")
        .battlefield(
            P0,
            &[Permanent::new(JACE).counters(CounterKind::Loyalty, 13)],
        )
        .battlefield(P1, &[Permanent::new(PRODIGY)])
        .hand(P1, &[PLAINS])
        .library(P0, &[ISLAND, ISLAND])
        .library(P1, &[ISLAND, ISLAND, ISLAND])
        .actions(&[
            Action::activate(P0, JACE, 2),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Hand,
                card: Some(PRODIGY),
                count: 1,
            }),
            loyalty(12),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::activate(P0, JACE, 3),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
            // Turn 2's draw left two Islands, now exiled; the hand of three is the library.
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Exile,
                card: Some(ISLAND),
                count: 2,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Library,
                card: None,
                count: 3,
            }),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Hand,
                card: None,
                count: 0,
            }),
            // Loyalty 0: put into its owner's graveyard (CR 704.5i).
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(JACE),
                count: 1,
            }),
        ]),
];
