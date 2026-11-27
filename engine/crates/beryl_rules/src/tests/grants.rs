//! The table of grants: one row for each effect that grants an ability, written as the
//! layer 6 changes apply ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters,
//! ability-removing effects, and effects that say an object can't have an ability are
//! applied." Each permanent has a bit for each row that grants it an ability.
//!
//! [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f

use beryl_card::effect::build::{draw, you};
use beryl_card::{Cost, Objects};

use super::LAND;
use super::cast::Game;
use crate::card::{Ability, CardDef, Face, LayerSpec, Modification, ObjectAbility, Types};
use crate::decision::{Choice, Decision};
use crate::filter::Filter;
use crate::fixed::Full;
use crate::ids::{Slot, SlotId};
use crate::replacement::event::Sink;
use crate::state::Zone;
use crate::{CardTable, Decklist, Large, Layout, Opt, Profile, continuous::layers, reference};

/// A vanilla 1/1 creature.
static GRUNT: CardDef = CardDef {
    name: "Test Grunt",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}: Draw a card."
static TAP_TO_DRAW: [Ability; 1] = [Ability::activated(Cost::TAP).effect(&draw(you(), 1))];

/// "Creatures have [the face's first grant]."
static CREATURES_HAVE: [Ability; 1] = [Ability::static_ability(&LayerSpec {
    affected: Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::Types(Types::CREATURE),
    },
    condition: None,
    modifications: &[Modification::grant(0)],
})];

/// "Creatures have '{T}: Draw a card.'"
static BANNER: CardDef = CardDef {
    name: "Test Banner",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &CREATURES_HAVE,
        grants: &TAP_TO_DRAW,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// The same as Test Banner, on a card of its own.
static PENNANT: CardDef = CardDef {
    name: "Test Pennant",
    ..BANNER
};

/// "Creatures lose all abilities."
static PLAIN: CardDef = CardDef {
    name: "Test Plain",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
            condition: None,
            modifications: &[Modification::LoseAllAbilities],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &GRUNT, &BANNER, &PENNANT, &PLAIN]);
const DECK: &Decklist = &[
    (4, "Test Grunt"),
    (20, "Test Banner"),
    (2, "Test Pennant"),
    (2, "Test Plain"),
    (20, "Test Land"),
];

/// A game at the first player's precombat main phase with a Test Grunt on the battlefield.
pub(super) fn grunt() -> (Game, Slot) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let grunt = g.give(g.me, "Test Grunt", Zone::Battlefield);
    (g, grunt)
}

/// Puts `n` of the first player's `card` onto the battlefield, one after another.
pub(super) fn give(g: &mut Game, card: &str, n: usize) {
    for _ in 0..n {
        g.give(g.me, card, Zone::Battlefield);
    }
}

/// Refreshes the layers and checks them against their references.
fn refresh(g: &mut Game) {
    layers::refresh(&mut g.s, &g.t, &mut Sink::off()).unwrap();
    reference::check_references(&g.s, &g.t).unwrap();
}

/// The abilities other objects granted the permanent in `slot`.
fn granted(g: &Game, slot: Slot) -> Vec<ObjectAbility> {
    (g.s.abilities(&g.t, slot))
        .map(|(ability, _)| ability)
        .filter(|a| matches!(a, ObjectAbility::Granted(_)))
        .collect()
}

/// The abilities of the permanent in `slot` a priority decision offers to activate.
fn offered(d: &Decision, slot: Slot) -> Vec<ObjectAbility> {
    (0..d.len() as u16)
        .filter_map(|i| match d.candidates.option(Choice(i)) {
            Some(Opt::Activate { slot: s, ability }) if s == slot => Some(ability),
            _ => None,
        })
        .collect()
}

/// Five permanents that each grant an ability give a creature five, in a `Small` game: the
/// table has a row for each and the creature a bit for each, where four slots once held them.
#[test]
fn five_grants_on_one_permanent_stay_in_small() {
    let (mut g, grunt) = grunt();
    give(&mut g, "Test Banner", 5);
    refresh(&mut g);
    assert_eq!(g.s.grant_rows().len(), 5);
    assert_eq!(
        granted(&g, grunt),
        (0..5).map(ObjectAbility::Granted).collect::<Vec<_>>()
    );
    assert!(g.s.has(grunt, ObjectAbility::Granted(4)));
    assert!(!g.s.has(grunt, ObjectAbility::Granted(5)));
}

/// A seventeenth row has no place in a `Small` table, and the recompute says so for the
/// game to go on in `Large`, where the same game has all seventeen.
#[test]
fn seventeen_rows_are_full_in_small_and_fit_in_large() {
    let (mut g, grunt) = grunt();
    give(&mut g, "Test Banner", 17);
    let mut large = g.s.widen();
    assert_eq!(layers::refresh(&mut g.s, &g.t, &mut Sink::off()), Err(Full));
    layers::refresh(&mut large, &g.t, &mut Sink::off()).unwrap();
    reference::check_references(&large, &g.t).unwrap();
    assert_eq!(large.grant_rows().len(), 17);
    let grunt: <Large as Layout>::Slot = SlotId::to(grunt).unwrap();
    assert!(large.has(grunt, ObjectAbility::Granted(16)));
}

/// Two effects that grant the same ability give two instances of it (see [CR 113.2c]): "If an
/// object has multiple instances of the same ability, each instance functions
/// independently." Activating either is activating the same ability, so the player is
/// offered it once, as the first granted, and the same ability from another card apart.
///
/// [CR 113.2c]: https://yawgatog.com/resources/magic-rules/#R1132c
#[test]
fn two_rows_of_one_grant_are_one_activation_candidate() {
    let (mut g, grunt) = grunt();
    give(&mut g, "Test Banner", 2);
    give(&mut g, "Test Pennant", 1);
    refresh(&mut g);
    assert_eq!(granted(&g, grunt).len(), 3);
    let d = g.decide();
    assert_eq!(
        offered(&d, grunt),
        [ObjectAbility::Granted(0), ObjectAbility::Granted(2)]
    );
}

/// An effect that removes all abilities removes those granted by effects with earlier
/// timestamps, and not those granted by later ones ([CR 613.7]): "An effect with an earlier
/// timestamp is applied before an effect with a later timestamp."
///
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
#[test]
fn losing_all_abilities_removes_only_the_grants_before_it() {
    let (mut g, grunt) = grunt();
    give(&mut g, "Test Banner", 1);
    give(&mut g, "Test Plain", 1);
    give(&mut g, "Test Pennant", 1);
    refresh(&mut g);
    // Both effects wrote a row; the Grunt has the later one's alone.
    assert_eq!(g.s.grant_rows().len(), 2);
    assert_eq!(granted(&g, grunt), [ObjectAbility::Granted(0)]);
    let pennant = g.s.granted_ability(grunt, 0).unwrap();
    assert_eq!(pennant.card, g.s.grant_rows()[1].card);
    assert_ne!(pennant.card, g.s.grant_rows()[0].card);
    let d = g.decide();
    assert_eq!(offered(&d, grunt), [ObjectAbility::Granted(0)]);
}
