//! Copies of spells (CR 707.10) and storm (CR 702.40), shown with test cards for what no card
//! of the pool does yet: a copy of a permanent spell, and storm far past what a scenario casts.

use beryl_card::effect::build::{gain_life, target_player};
use beryl_card::{Amount, Effect, Objects, TargetKind, TargetSpec};

use super::LAND;
use super::cast::{Game, cast};
use crate::card::{Ability, CardDef, Face, Types};
use crate::filter::Filter;
use crate::state::{Count, Zone};
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile};

/// A 2/2 creature that costs nothing.
static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Copy target creature spell."
static TWIN: CardDef = CardDef {
    name: "Test Twin",
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .target(&TargetSpec::one(
                TargetKind::Spell,
                Filter::Types(Types::CREATURE),
            ))
            .effect(&Effect::CopySpell {
                spells: Objects::Target(beryl_card::SpecIx::new(0)),
                count: Amount::Fixed(1),
                except: None,
                new_targets: false,
            })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Target player gains 1 life. Storm"
static SURGE: CardDef = CardDef {
    name: "Test Surge",
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[
            Ability::spell()
                .target(&TargetSpec::player())
                .effect(&gain_life(target_player(), 1)),
            Ability::storm(),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &TWIN, &SURGE]);
const DECK: &Decklist = &[
    (30, "Test Land"),
    (2, "Test Bear"),
    (1, "Test Twin"),
    (1, "Test Surge"),
];

/// A copy of a permanent spell becomes a token as it resolves, and stays on the battlefield
/// ([CR 608.3f]): "If the object that's resolving is a copy of a permanent spell, it will
/// become a token permanent as it is put onto the battlefield". It is no card, so it is not the
/// spell's card twice, and the spell resolves on its own after it.
///
/// [CR 608.3f]: https://yawgatog.com/resources/magic-rules/#R6083f
#[test]
fn a_copy_of_a_creature_spell_becomes_a_token() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let twin = g.give(g.me, "Test Twin", Zone::Hand);
    g.pick(cast(bear));
    // The bear's caster holds priority and copies it: Twin's one target is the bear.
    g.pick(cast(twin));
    assert_eq!(g.s.stack().len(), 2);
    g.pass_both();
    assert_eq!(g.s.stack().len(), 2, "the bear and its copy");
    let copy = g.s.stack().last().unwrap().spell().unwrap().slot;
    assert!(g.s.object(copy).is_token());
    assert_eq!(g.s.controller(copy), g.me);
    g.pass_both();
    assert_eq!(
        g.s.object(copy).zone(),
        Zone::Battlefield,
        "a token permanent"
    );
    assert!(g.s.object(copy).is_token());
    g.pass_both();
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
    assert!(!g.s.object(bear).is_token());
    assert!(g.s.stack().is_empty());
}

/// Storm after twenty spells makes twenty copies in the small layout, whose stack holds 64
/// entries, and each copy's controller is asked about its target without the game stalling
/// ([CR 702.40a]): "copy it for each other spell that was cast before it this turn. If the spell
/// has any targets, you may choose new targets for any of the copies."
///
/// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
#[test]
fn storm_makes_twenty_copies_in_the_small_layout() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let surge = g.give(g.me, "Test Surge", Zone::Hand);
    // Twenty spells were cast this turn before it.
    g.s.board().set_counted(g.me, Count::SpellsCast, 20);
    g.pick(cast(surge));
    g.pick(Opt::Player(g.me));
    let mut asked = 0;
    let mut deepest = 0;
    while g.s.outcome().is_none() && !(g.s.stack().is_empty() && g.s.triggers().is_empty()) {
        deepest = deepest.max(g.s.stack().len());
        let d = g.current();
        if d.kind == DecisionKind::Target {
            asked += 1;
        }
        // Each copy keeps its target, the first option.
        g.choose(Choice(0));
    }
    assert_eq!(g.s.outcome(), None);
    assert_eq!(asked, 20, "one question for each copy's target");
    assert_eq!(deepest, 21, "the spell and its twenty copies");
    assert_eq!(g.s.player(g.me).life(), 41);
    assert_eq!(
        g.s.counted(Count::SpellsCast, g.me),
        21,
        "copies are not cast"
    );
}
