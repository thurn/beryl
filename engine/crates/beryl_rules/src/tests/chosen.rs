//! A value a permanent chooses as it enters: a color, a creature type or a card name
//! ([CR 614.12a]): "If a replacement effect that modifies how a permanent enters the
//! battlefield requires a choice, that choice is made before the permanent enters the
//! battlefield." The permanent remembers it while it stays.
//!
//! [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a

use beryl_card::{ChosenKind, Colors, Subtype, Subtypes};
use beryl_kernel::chosen::{ANOTHER, ChosenValue};

use crate::card::{Ability, CardDef, Face, Purpose, Replacement, Types};
use crate::scratch::Scratch;
use crate::state::{Pending, Zone, ZoneKey};
use crate::zone::Place;
use crate::{
    Answers, CardTable, Choice, Decision, DecisionKind, GameTable, Opt, Profile, Replay, Seat,
    Slot, State, Step, apply, next,
};
use beryl_kernel::format::Format;

use super::begin;
use super::cast::{cast, index_of};

/// "As this artifact enters, choose a color."
pub(super) static PAINTER: CardDef = CardDef {
    name: "Test Painter",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::replacing(&Replacement::choose_as_enters(
            ChosenKind::Color,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "As this artifact enters, choose a card name."
pub(super) static NEEDLE: CardDef = CardDef {
    name: "Test Needle",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::replacing(&Replacement::choose_as_enters(
            ChosenKind::CardName,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "As this land enters, choose a creature type."
pub(super) static CAVERN: CardDef = CardDef {
    name: "Test Cavern",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(&Replacement::choose_as_enters(
            ChosenKind::CreatureType,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature whose type the game numbers, and the token it makes, whose name no card has.
pub(super) static GOBLIN: CardDef = CardDef {
    name: "Test Goblin",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Goblin])),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[&GOBLIN_TOKEN],
};

static GOBLIN_TOKEN: CardDef = CardDef {
    name: "Test Goblin Token",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Goblin])),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

pub(super) static LAND: CardDef = CardDef {
    name: "Test Plain Land",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};

pub(super) static CARDS: CardTable = CardTable::new(&[&PAINTER, &NEEDLE, &CAVERN, &GOBLIN, &LAND]);

pub(super) const DECK: &[(u8, &str)] = &[
    (4, "Test Painter"),
    (4, "Test Needle"),
    (4, "Test Cavern"),
    (4, "Test Goblin"),
    (24, "Test Plain Land"),
];

/// A game in the starting player's first main phase, with that player.
fn game() -> (GameTable, State, Seat) {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 5, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    (t, s, p)
}

/// Moves a copy of `name` from `player`'s library to their hand.
pub(super) fn to_hand(t: &GameTable, s: &mut State, player: Seat, name: &str) -> Slot {
    let slot = (crate::tests::fixture_slots(s.zone(ZoneKey::Of(player, Zone::Library))))
        .find(|&slot| s.definition_name(t, slot) == name)
        .unwrap();
    s.move_object(
        &mut crate::replacement::event::Sink::off(),
        slot,
        Zone::Hand,
        Place::Top,
    )
    .unwrap();
    slot
}

pub(super) fn decide(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    a: &mut dyn Answers,
) -> Decision {
    match next(s, t, x, a) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    }
}

/// Casts `slot`, a spell with no cost, and passes until a decision other than priority comes
/// or the stack is empty.
fn cast_and_pass(t: &GameTable, s: &mut State, x: &mut Scratch, slot: Slot) -> Decision {
    let d = decide(t, s, x, &mut Replay);
    apply(
        s,
        t,
        x,
        &mut Replay,
        index_of(&d, cast(slot)).expect("castable"),
    );
    loop {
        let d = decide(t, s, x, &mut Replay);
        if d.kind != DecisionKind::Priority || s.stack().is_empty() {
            return d;
        }
        apply(s, t, x, &mut Replay, index_of(&d, Opt::Pass).unwrap());
    }
}

/// The choice of `d` that names `value`.
fn choice_of(d: &Decision, value: ChosenValue) -> Choice {
    index_of(d, Opt::Number(value.number())).expect("a candidate for the value")
}

/// A permanent spell asks its controller for a color as it resolves, among the five, and the
/// permanent enters remembering the one chosen.
#[test]
fn a_permanent_spell_chooses_a_color_as_it_enters() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let painter = to_hand(&t, &mut s, p, "Test Painter");
    let d = cast_and_pass(&t, &mut s, &mut x, painter);
    assert_eq!((d.prompt.purpose, d.player), (Purpose::Color, p));
    assert!(matches!(s.pending(), Pending::Resolving(_)));
    let offered: Vec<_> = (0..d.len() as u16)
        .map(|i| d.candidates.option(Choice(i)))
        .collect();
    let numbers: Vec<_> = (0..5).map(|n| Some(Opt::Number(n))).collect();
    assert_eq!(offered, numbers, "the five colors, in the rules' order");
    assert_eq!(s.object(painter).zone(), Zone::Stack, "not yet entered");
    let red = ChosenValue::Color(Colors::RED);
    apply(&mut s, &t, &mut x, &mut Replay, choice_of(&d, red));
    assert_eq!(s.object(painter).zone(), Zone::Battlefield);
    assert_eq!(s.chosen(painter), Some(red));
}

/// A land played asks for a creature type before it enters: the types the game's cards name,
/// and one for every other type.
#[test]
fn a_played_land_chooses_a_creature_type() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let cavern = to_hand(&t, &mut s, p, "Test Cavern");
    let d = decide(&t, &mut s, &mut x, &mut Replay);
    let play = index_of(&d, Opt::PlayLand(cavern)).unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, play);
    let d = decide(&t, &mut s, &mut x, &mut Replay);
    assert_eq!((d.prompt.purpose, d.player), (Purpose::CreatureType, p));
    assert_eq!(s.object(cavern).zone(), Zone::Hand, "not yet entered");
    let goblin = t
        .subtypes()
        .creature_types()
        .find(|&(_, st)| st == Subtype::Goblin);
    let goblin = ChosenValue::CreatureType(Some(goblin.unwrap().0));
    let names: Vec<String> = (0..d.len() as u16)
        .map(|i| match d.candidates.option(Choice(i)) {
            Some(Opt::Number(n)) => {
                ChosenValue::<crate::layout::Small>::of_number(ChosenKind::CreatureType, n)
                    .unwrap()
                    .name(&t)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(names, ["Goblin", "Another creature type"]);
    apply(&mut s, &t, &mut x, &mut Replay, choice_of(&d, goblin));
    assert_eq!(s.object(cavern).zone(), Zone::Battlefield);
    assert_eq!(s.chosen(cavern), Some(goblin));
}

/// A name is one of the game's cards, each once, or one choice for every other name: a token's
/// name is not a card's.
#[test]
fn a_card_name_is_one_of_the_games_cards_or_another() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let needle = to_hand(&t, &mut s, p, "Test Needle");
    let d = cast_and_pass(&t, &mut s, &mut x, needle);
    assert_eq!(d.prompt.purpose, Purpose::CardName);
    let names: Vec<String> = (0..d.len() as u16)
        .map(|i| match d.candidates.option(Choice(i)) {
            Some(Opt::Number(n)) => {
                ChosenValue::<crate::layout::Small>::of_number(ChosenKind::CardName, n)
                    .unwrap()
                    .name(&t)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "Test Painter",
            "Test Needle",
            "Test Cavern",
            "Test Goblin",
            "Test Plain Land",
            "Another card",
        ]
    );
    let another = ChosenValue::Name(None);
    assert_eq!(another.number(), ANOTHER);
    apply(&mut s, &t, &mut x, &mut Replay, choice_of(&d, another));
    assert_eq!(s.chosen(needle), Some(another));
}

/// Two permanents of one card that chose different colors are told apart, and two that chose
/// the same are alike ([`State::signature`]).
#[test]
fn the_signature_tells_apart_permanents_that_chose_differently() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let mut painters = Vec::new();
    for color in [Colors::RED, Colors::BLUE, Colors::RED] {
        let painter = to_hand(&t, &mut s, p, "Test Painter");
        let d = cast_and_pass(&t, &mut s, &mut x, painter);
        let value = ChosenValue::Color(color);
        apply(&mut s, &t, &mut x, &mut Replay, choice_of(&d, value));
        assert_eq!(s.chosen(painter), Some(value));
        painters.push(painter);
    }
    let [red, blue, other_red] = painters[..] else {
        unreachable!()
    };
    assert_ne!(s.signature(red), s.signature(blue));
    assert_eq!(s.signature(red), s.signature(other_red));
}
