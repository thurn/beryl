//! Effect data against Rust bodies (plan K1): a card defined once each way plays its
//! scenarios to the same events and the same states.
//!
//! Each twin here keeps its own copy of the card's Rust body, so the comparison stands
//! whichever way the card's own file is written.

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_card::effect::build::{
    ITS_CONTROLLER, TARGET, YOU, counter, create_tokens, destroy, draw, exile_until_this_leaves,
    gain_life, put_into_library, return_to_hand, sequence, unless_pays,
};
use beryl_card::{Effect, LibraryPosition, SpecIx};
use beryl_engine::card::{Ability, Behavior, CardDef, Face, Flow, ManaCost, TargetSpec, mana};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::{Event, TokenEntry};
use beryl_engine::state::TargetPlace;
use beryl_engine::{CardTable, Layout, State, bodies, coverage};

use crate::white::{get_lost, mana_tithe, oust, portable_hole, reprieve};
use crate::{CARDS, SCENARIOS};

/// A card whose first ability is written twice.
struct Twin {
    card: &'static CardDef,
    body: Behavior,
    /// What the ability declares beside its body, which data says itself.
    declares: Declares,
    data: &'static Effect,
}

/// What an ability with a Rust body declares by hand.
#[derive(Clone, Copy, Default)]
struct Declares {
    /// That resolving it may ask a player to choose.
    may_pause: bool,
    /// That it removes the permanent it targets.
    removal: bool,
}

const NOTHING: Declares = Declares {
    may_pause: false,
    removal: false,
};
const MAY_PAUSE: Declares = Declares {
    may_pause: true,
    removal: false,
};
const REMOVAL: Declares = Declares {
    may_pause: false,
    removal: true,
};

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

/// Reads the controller before the move: in a library the creature has none.
fn second_from_the_top<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(creature) = ctx.target_object(TargetPlace::FIRST) else {
        return Ok(());
    };
    let controller = ctx.state().controller(creature.slot);
    ctx.put_into_library(creature, 1);
    ctx.gain_life(controller, 3);
    Ok(())
}

fn destroy_and_map<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let Some(permanent) = ctx.target_object(TargetPlace::FIRST) else {
        return Ok(());
    };
    let controller = ctx.state().controller(permanent.slot);
    ctx.destroy(permanent);
    ctx.create_tokens_for(controller, &get_lost::MAP, 2, TokenEntry::default());
    Ok(())
}

fn exile_until_it_leaves<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(permanent) = ctx.target_object(TargetPlace::FIRST) {
        ctx.exile_until_source_leaves(permanent);
    }
    Ok(())
}

fn return_and_draw<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(spell) = ctx.target_object(TargetPlace::FIRST) {
        ctx.return_to_hand(spell);
    }
    ctx.draw(ctx.controller(), 1);
    Ok(())
}

/// A static of its own: it names another, the token's definition.
static DESTROY_AND_MAP: Effect = sequence(&[
    destroy(TARGET),
    create_tokens(ITS_CONTROLLER, &get_lost::MAP, 2),
]);

const TWINS: &[Twin] = &[
    // "Counter target spell unless its controller pays {1}."
    Twin {
        card: &mana_tithe::CARD,
        body: bodies!(counter_unless_pays_one),
        declares: MAY_PAUSE,
        data: &unless_pays(&counter(TARGET), ITS_CONTROLLER, mana("{1}")),
    },
    // "Put target creature into its owner's library second from the top. Its controller
    // gains 3 life."
    Twin {
        card: &oust::CARD,
        body: bodies!(second_from_the_top),
        declares: REMOVAL,
        data: &sequence(&[
            put_into_library(TARGET, LibraryPosition::FromTop(2)),
            gain_life(ITS_CONTROLLER, 3),
        ]),
    },
    // "Destroy target creature, enchantment, or planeswalker. Its controller creates two Map
    // tokens."
    Twin {
        card: &get_lost::CARD,
        body: bodies!(destroy_and_map),
        declares: REMOVAL,
        data: &DESTROY_AND_MAP,
    },
    // "When this artifact enters, exile target nonland permanent an opponent controls with
    // mana value 2 or less until this artifact leaves the battlefield."
    Twin {
        card: &portable_hole::CARD,
        body: bodies!(exile_until_it_leaves),
        declares: REMOVAL,
        data: &exile_until_this_leaves(TARGET),
    },
    // "Return target spell to its owner's hand. Draw a card."
    Twin {
        card: &reprieve::CARD,
        body: bodies!(return_and_draw),
        declares: NOTHING,
        data: &sequence(&[return_to_hand(TARGET), draw(YOU, 1)]),
    },
];

/// `card` with `body` for its first ability's, declaring `declares` beside it whatever the
/// card's own file does: an ability written as data declares nothing, as data says both
/// itself.
fn written_as(card: &'static CardDef, body: Behavior, declares: Declares) -> &'static CardDef {
    let front = &card.faces[0];
    let original = &front.abilities[0];
    let targets: Vec<TargetSpec> = (original.targets.iter())
        .map(|&spec| TargetSpec {
            removes_permanent: declares.removal,
            ..spec
        })
        .collect();
    let ability = Ability {
        body,
        may_pause: declares.may_pause,
        targets: targets.leak(),
        ..*original
    };
    let face = Face {
        abilities: Box::leak(Box::new([ability])),
        ..*front
    };
    Box::leak(Box::new(CardDef {
        name: card.name,
        faces: Box::leak(Box::new([face])),
        tokens: card.tokens,
    }))
}

/// Every card, with `twin` in place of the card it is written from.
fn table_with(twin: &'static CardDef) -> &'static CardTable {
    let cards: Vec<&'static CardDef> = (CARDS.cards().iter())
        .map(|&card| if card.name == twin.name { twin } else { card })
        .collect();
    Box::leak(Box::new(CardTable::new(cards.leak())))
}

/// What a scenario did: the state each action began from, hashed, with the events since the
/// action before.
fn played(
    cards: &'static CardTable,
    scenario: &beryl_scenario::Scenario,
) -> Vec<(u64, Vec<Event>)> {
    let mut log = Vec::new();
    let run = beryl_scenario::watch(cards, scenario, |s: &State, events| {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        log.push((hasher.finish(), events.to_vec()));
    });
    run.unwrap_or_else(|failure| panic!("{failure}"));
    log
}

#[test]
fn a_card_written_as_data_plays_its_scenarios_as_its_body_does() {
    for twin in TWINS {
        let name = twin.card.name;
        let as_body = written_as(twin.card, twin.body, twin.declares);
        let as_data = written_as(twin.card, Behavior::Data(twin.data), NOTHING);
        let (bodies, data) = (table_with(as_body), table_with(as_data));
        let scenarios = (SCENARIOS.iter())
            .find(|&&(card, _)| card == name)
            .map(|&(_, scenarios)| scenarios)
            .unwrap();
        assert!(!scenarios.is_empty(), "{name} has no scenarios");
        coverage::start();
        for scenario in scenarios {
            let (expected, got) = (played(bodies, scenario), played(data, scenario));
            assert_eq!(expected.len(), got.len(), "{}", scenario.name);
            for (i, (expected, got)) in expected.iter().zip(&got).enumerate() {
                assert_eq!(
                    expected.1, got.1,
                    "{}: events before action {i}",
                    scenario.name
                );
                assert_eq!(
                    expected.0, got.0,
                    "{}: the state at action {i}",
                    scenario.name
                );
            }
        }
        // Each definition's ability resolved: the scenarios compared what they are for, and
        // coverage records an ability written as data as it records a body.
        let resolved = coverage::take();
        for def in [as_body, as_data] {
            let ability = coverage::address(&def.faces[0].abilities[0]);
            assert!(resolved.contains(&ability), "{name}'s ability never ran");
        }
        // What the data says, its card need not declare.
        let (body, data) = (
            &as_body.faces[0].abilities[0],
            &as_data.faces[0].abilities[0],
        );
        assert_eq!(data.asks(), body.asks(), "{name}: whether it may pause");
        let (declared, derived) = (
            body.removes_target(SpecIx::new(0)),
            data.removes_target(SpecIx::new(0)),
        );
        assert_eq!(derived, declared, "{name}: whether it is removal");
    }
}
