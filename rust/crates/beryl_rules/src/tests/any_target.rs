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
/// starting player has cast a Test Bolt and is choosing its target. Returns the game and
/// the starting player's and the opponent's Bears.
fn bolting() -> (Game, Slot, Slot) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let (me, them) = (g.me, g.them);
    let mine = g.give(me, "Test Bear", Zone::Battlefield);
    let theirs = g.give(them, "Test Bear", Zone::Battlefield);
    g.give(me, "Test Land", Zone::Battlefield);
    g.give(them, "Test Land", Zone::Battlefield);
    let bolt = g.give(me, "Test Bolt", Zone::Hand);
    g.pick(cast(bolt));
    (g, mine, theirs)
}

#[test]
fn the_target_step_offers_every_player_and_every_creature() {
    let (mut g, mine, theirs) = bolting();
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
