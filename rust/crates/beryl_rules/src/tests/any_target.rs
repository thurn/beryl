//! "Any target" ([CR 115.4]): "Some spells and abilities that refer to damage require "any
//! target," "another target," "two targets," or similar rather than "target [something]."
//! These targets may be creatures, players, planeswalkers, or battles." Damage to the target,
//! a player or a permanent, is dealt by the one instruction.
//!
//! [CR 115.4]: https://yawgatog.com/resources/magic-rules/#R1154

use beryl_card::effect::build::{deal_damage_to_any_target, sequence, that_many, you};
use beryl_card::{Effect, TargetSpec};

use super::LAND;
use super::cast::{BEAR, Game, cast};
use crate::card::{Ability, CardDef, Face, Types};
use crate::ids::Slot;
use crate::replacement::event::{self, Reason};
use crate::state::{Continuous, Duration, Shield, Subject, Zone};
use crate::{CardTable, DecisionKind, Decklist, Opt, Profile};

/// "Test Bolt deals 3 damage to any target", for no mana.
static BOLT: CardDef = CardDef {
    name: "Test Bolt",
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::any_target()])
            .effect(&deal_damage_to_any_target(3))],
        ..Face::EMPTY
    }],
};

/// "Test Drain deals 3 damage to any target. You gain life equal to the damage dealt this
/// way", for no mana.
static DRAIN: CardDef = CardDef {
    name: "Test Drain",
    tokens: &[],
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::any_target()])
            .effect(&sequence(&[
                deal_damage_to_any_target(3),
                Effect::GainLife {
                    player: you(),
                    amount: that_many(),
                },
            ]))],
        ..Face::EMPTY
    }],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &BOLT, &DRAIN]);
const DECK: &Decklist = &[
    (30, "Test Land"),
    (4, "Test Bear"),
    (3, "Test Bolt"),
    (3, "Test Drain"),
];

/// A game where each player has a Test Bear and a Test Land on the battlefield, and the
/// starting player has cast a Test Bolt and is choosing its target. Returns the game, the
/// Bolt, and the starting player's and the opponent's Bears.
fn bolting() -> (Game, Slot, Slot, Slot) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let (me, them) = (g.me, g.them);
    let mine = g.give(me, "Test Bear", Zone::Battlefield);
    let theirs = g.give(them, "Test Bear", Zone::Battlefield);
    g.give(me, "Test Land", Zone::Battlefield);
    g.give(them, "Test Land", Zone::Battlefield);
    let bolt = g.give(me, "Test Bolt", Zone::Hand);
    g.pick(cast(bolt));
    (g, bolt, mine, theirs)
}

#[test]
fn the_target_step_offers_every_player_and_every_creature() {
    let (mut g, _, mine, theirs) = bolting();
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Target);
    let offered: Vec<Opt> = (0..d.len())
        .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
        .collect();
    // The players first, by seat, then the creatures; no land, which is no creature.
    let players = crate::ids::Seat::BOTH.map(Opt::Player);
    assert_eq!(&offered[..2], &players);
    let mut objects = offered[2..].to_vec();
    objects.sort_by_key(|o| format!("{o:?}"));
    let mut expected = vec![Opt::Object(mine), Opt::Object(theirs)];
    expected.sort_by_key(|o| format!("{o:?}"));
    assert_eq!(objects, expected);
}

#[test]
fn damage_to_a_player_lowers_their_life() {
    let (mut g, bolt, mine, theirs) = bolting();
    g.pick(Opt::Player(g.them));
    g.pass_both();
    assert_eq!(g.s.player(g.them).life(), 17);
    assert_eq!(g.s.player(g.me).life(), 20);
    assert_eq!(g.s.object(bolt).zone(), Zone::Graveyard);
    for bear in [mine, theirs] {
        assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
    }
}

/// Damage to a creature is marked on it, and a creature with lethal damage marked on it is
/// destroyed the next time state-based actions are checked ([CR 704.5g]): "If a creature has
/// toughness greater than 0, it has damage marked on it, and the total damage marked on it is
/// greater than or equal to its toughness, that creature has been dealt lethal damage and is
/// destroyed."
///
/// [CR 704.5g]: https://yawgatog.com/resources/magic-rules/#R7045g
#[test]
fn damage_to_a_creature_destroys_it() {
    let (mut g, _, mine, theirs) = bolting();
    g.pick(Opt::Object(theirs));
    g.pass_both();
    g.decide();
    assert_eq!(g.s.object(theirs).zone(), Zone::Graveyard);
    assert_eq!(g.s.object(mine).zone(), Zone::Battlefield);
    assert_eq!(g.s.player(g.them).life(), 20);
}

/// A target that has left the battlefield is illegal, and a spell whose every target is
/// illegal doesn't resolve ([CR 608.2b]): "If all its targets, for every instance of the word
/// "target," are now illegal, the spell or ability doesn't resolve."
///
/// [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
#[test]
fn a_target_that_left_makes_the_spell_do_nothing() {
    let (mut g, bolt, _, theirs) = bolting();
    g.pick(Opt::Object(theirs));
    let sink = &mut g.x.events;
    event::move_one(&mut g.s, &g.t, sink, theirs, Zone::Hand, Reason::Effect).unwrap();
    g.pass_both();
    assert_eq!(g.s.object(bolt).zone(), Zone::Graveyard);
    assert_eq!(g.s.player(g.them).life(), 20);
    assert_eq!(g.s.player(g.me).life(), 20);
}

/// Damage a prevention shield prevents is not dealt ([CR 615.1]): prevention effects "apply to
/// damage and modify how much damage, if any, is dealt". So "the damage dealt this way" counts
/// only what got past the shield: 3 damage at a player with a shield of 2 is 1 dealt, and 1
/// life gained.
///
/// [CR 615.1]: https://yawgatog.com/resources/magic-rules/#R6151
#[test]
fn damage_dealt_this_way_counts_none_that_was_prevented() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let (me, them) = (g.me, g.them);
    let drain = g.give(me, "Test Drain", Zone::Hand);
    let card = g.s.copiable(drain).record(&g.t).0;
    let shield = Shield {
        left: Some(2),
        from: None,
    };
    let prevent = Continuous::Prevent(Subject::Player(them), shield);
    g.s.start_effect(card, prevent, Duration::EndOfTurn)
        .unwrap();
    g.pick(cast(drain));
    g.pick(Opt::Player(them));
    g.pass_both();
    assert_eq!(g.s.player(them).life(), 19);
    assert_eq!(g.s.player(me).life(), 21);
}
