//! Static rule changes to what a spell costs, tested directly with test-only cards
//! ([CR 601.2f]): "The total cost is the mana cost or
//! alternative cost (as determined in rule 601.2b), plus all additional costs and cost
//! increases, and minus all cost reductions."
//!
//! [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f

use beryl_card::{ManaCost, Players, RuleChange, SpellCost};

use crate::card::{Ability, CardDef, Face, Types};
use crate::filter::Filter;
use crate::ids::Seat;
use crate::state::{TurnStep, Zone};
use crate::{CardTable, GameTable, Profile, State, continuous::layers, continuous::rule_change};
use beryl_kernel::format::Format;

use super::LAND;
use super::events::find;

/// "Spells you cast cost {2} less to cast."
static DISCOUNT_RULE: RuleChange =
    RuleChange::spell_costs(Players::You, Filter::Any, SpellCost::Less(2));
/// "Instant spells your opponents cast cost {1} more to cast."
static TAX_RULE: RuleChange = RuleChange::spell_costs(
    Players::EachOpponent,
    Filter::Types(Types::INSTANT),
    SpellCost::More(1),
);

static DISCOUNT: CardDef = CardDef {
    name: "Test Discount",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&DISCOUNT_RULE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static TAX: CardDef = CardDef {
    name: "Test Tax",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&TAX_RULE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Each spell that would cost less than three mana to cast costs three mana to cast."
static FLOOR_RULE: RuleChange =
    RuleChange::spell_costs(Players::EachPlayer, Filter::Any, SpellCost::AtLeast(3));
static FLOOR: CardDef = CardDef {
    name: "Test Floor",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&FLOOR_RULE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static CARDS: CardTable = CardTable::new(&[&LAND, &DISCOUNT, &TAX, &FLOOR]);

/// An instant and a sorcery, as faces a rule change is asked about.
static INSTANT: Face = Face {
    types: Types::INSTANT,
    ..Face::EMPTY
};
static SORCERY: Face = Face {
    types: Types::SORCERY,
    ..Face::EMPTY
};

/// A game in which P0 controls `card`.
fn with_permanent_of(card: &'static str) -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[(1, card), (9, "Test Land")];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let permanent = find(&s, &t, Seat::P0, card);
    s.board().place(permanent, Zone::Battlefield).unwrap();
    layers::refresh(&mut s, &t, &mut crate::replacement::event::Sink::off()).unwrap();
    (t, s)
}

/// {`generic`}{U}.
fn blue(generic: u8) -> ManaCost {
    ManaCost::new(generic, [0, 1, 0, 0, 0, 0])
}

/// A reduction takes generic mana only, and never below none ([CR 601.2f]): "If the mana
/// component of the total cost is reduced to nothing by cost reduction effects, it is
/// considered to be {0}. It can't be reduced to less than {0}." It is for "you", the
/// controller of the permanent.
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
#[test]
fn a_reduction_takes_generic_mana_down_to_none() {
    let (t, s) = with_permanent_of("Test Discount");
    let cost = |player, generic| rule_change::spell_cost(&s, &t, player, &SORCERY, blue(generic));
    assert_eq!(cost(Seat::P0, 3), blue(1));
    assert_eq!(cost(Seat::P0, 1), blue(0));
    assert_eq!(cost(Seat::P1, 3), blue(3));
}

/// An increase is for the players and the spells it names: here each opponent of the
/// permanent's controller, and instants.
#[test]
fn an_increase_is_for_the_players_and_spells_it_names() {
    let (t, s) = with_permanent_of("Test Tax");
    let cost = |player, face| rule_change::spell_cost(&s, &t, player, face, blue(1));
    assert_eq!(cost(Seat::P1, &INSTANT), blue(2));
    assert_eq!(cost(Seat::P1, &SORCERY), blue(1));
    assert_eq!(cost(Seat::P0, &INSTANT), blue(1));
}

/// A floor counts the whole total, X at its value as the total already has it, and makes up
/// the difference in generic mana: a spell cast for no mana costs three, and one that costs
/// three or more already costs what it did.
#[test]
fn a_floor_makes_up_the_total_in_generic_mana() {
    let (t, s) = with_permanent_of("Test Floor");
    let cost = |player, cost| rule_change::spell_cost(&s, &t, player, &SORCERY, cost);
    assert_eq!(cost(Seat::P1, ManaCost::NONE), ManaCost::generic(3));
    assert_eq!(cost(Seat::P0, blue(0)), blue(2));
    assert_eq!(cost(Seat::P0, blue(2)), blue(2));
    assert_eq!(cost(Seat::P0, blue(4)), blue(4));
}
