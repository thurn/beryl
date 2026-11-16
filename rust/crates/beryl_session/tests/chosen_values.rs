//! A value a permanent chooses as it enters, in a save: the answer is recorded by the value's
//! name, so a replay finds it in a game whose decklists number the cards and creature types
//! otherwise.

use beryl_engine::card::{
    Ability, CardDef, ChosenKind, Face, Purpose, Replacement, Subtype, Subtypes, Types,
};
use beryl_engine::chosen::ChosenValue;
use beryl_engine::event::Sink;
use beryl_engine::profile::Profile;
use beryl_engine::state::{Zone, ZoneKey};
use beryl_engine::table::{CardTable, Decklist};
use beryl_engine::zone::Place;
use beryl_engine::{
    Choice, Decision, DecisionKind, GameTable, Opt, Replay, Scratch, State, Step, apply, next,
};
use beryl_session::save::{Act, Chosen};

static NEEDLE: CardDef = CardDef {
    name: "Test Needle",
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::replacing(&Replacement::choose_as_enters(
            ChosenKind::CardName,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CAVERN: CardDef = CardDef {
    name: "Test Cavern",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::replacing(&Replacement::choose_as_enters(
            ChosenKind::CreatureType,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static GOBLIN: CardDef = CardDef {
    name: "Test Goblin",
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: beryl_card::SubtypeWords::new(Subtypes::of(&[Subtype::Goblin])),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static ELF: CardDef = CardDef {
    name: "Test Elf",
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: beryl_card::SubtypeWords::new(Subtypes::of(&[Subtype::Elf])),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static LAND: CardDef = CardDef {
    name: "Test Plain Land",
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&NEEDLE, &CAVERN, &GOBLIN, &ELF, &LAND]);

/// The game's cards with the creatures first, so the Goblin is game index 0 and Goblin is
/// the first creature type numbered after Elf.
const CREATURES_FIRST: &Decklist = &[
    (4, "Test Goblin"),
    (4, "Test Elf"),
    (4, "Test Needle"),
    (4, "Test Cavern"),
    (24, "Test Plain Land"),
];

/// The same cards in another order, which numbers them otherwise.
const NEEDLE_FIRST: &Decklist = &[
    (4, "Test Needle"),
    (4, "Test Cavern"),
    (4, "Test Elf"),
    (4, "Test Goblin"),
    (24, "Test Plain Land"),
];

fn decide(t: &GameTable, s: &mut State, x: &mut Scratch) -> Decision {
    match next(s, t, x, &mut Replay) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    }
}

fn find(d: &Decision, opt: Opt) -> Choice {
    (0..d.len() as u16)
        .map(Choice)
        .find(|&c| d.candidates.option(c) == Some(opt))
        .unwrap_or_else(|| panic!("no {opt:?}"))
}

/// A game of `deck` in its starting player's first main phase, with a copy of `card` moved to
/// that player's hand and taken: played if a land, cast otherwise, and passed on until the
/// value it chooses is asked. Returns the question.
fn asked(deck: &'static Decklist, card: &str) -> (GameTable, State, Decision) {
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 11, Profile::EXACT).unwrap();
    let mut x = Scratch::new();
    let mut d = decide(&t, &mut s, &mut x);
    while matches!(
        d.kind,
        DecisionKind::PlayOrDraw | DecisionKind::KeepOrMulligan
    ) {
        apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
        d = decide(&t, &mut s, &mut x);
    }
    let p = s.turn().starting_player();
    let slot = (beryl_engine::Slot::all()
        .filter(|&slot| (s.zone(ZoneKey::Of(p, Zone::Library))).contains(slot)))
    .find(|&slot| s.definition_name(&t, slot) == card)
    .unwrap();
    s.move_object(&mut Sink::off(), slot, Zone::Hand, Place::Top)
        .unwrap();
    let d = decide(&t, &mut s, &mut x);
    let take = (0..d.len() as u16).map(Choice).find(|&c| {
        matches!(d.candidates.option(c), Some(Opt::PlayLand(o) | Opt::Cast { slot: o, .. }) if o == slot)
    });
    apply(&mut s, &t, &mut x, &mut Replay, take.unwrap());
    loop {
        let d = decide(&t, &mut s, &mut x);
        if d.kind != DecisionKind::Priority {
            return (t, s, d);
        }
        apply(&mut s, &t, &mut x, &mut Replay, find(&d, Opt::Pass));
    }
}

/// The answer to `d` that names the value called `name`.
fn named(t: &GameTable, d: &Decision, kind: ChosenKind, name: &str) -> Choice {
    (0..d.len() as u16)
        .map(Choice)
        .find(|&c| match d.candidates.option(c) {
            Some(Opt::Number(n)) => ChosenValue::<beryl_engine::Small>::of_number(kind, n)
                .is_some_and(|value| value.name(t) == name),
            _ => false,
        })
        .unwrap_or_else(|| panic!("no {name}"))
}

/// A card name is saved as the name and found in a game that numbers the card otherwise.
#[test]
fn a_chosen_name_is_saved_by_name_and_replays_where_the_cards_are_numbered_otherwise() {
    let (t, s, d) = asked(CREATURES_FIRST, "Test Needle");
    assert_eq!(d.prompt.purpose, Purpose::CardName);
    let c = named(&t, &d, ChosenKind::CardName, "Test Goblin");
    let saved = Chosen::new(&t, &s, &d, c);
    let what = saved.what.as_ref().unwrap();
    assert_eq!(
        what.act,
        Act::Chosen {
            value: "Test Goblin".to_owned()
        }
    );
    let file = serde_json::to_string(&saved).unwrap();
    assert!(
        file.contains(r#""act":"chosen","value":"Test Goblin""#),
        "{file}"
    );
    let loaded: Chosen = serde_json::from_str(&file).unwrap();
    assert_eq!(loaded.find(&t, &s, &d), Some(c));

    // Another table numbers the Goblin otherwise, and the answer finds it by its name.
    let (other_t, other_s, other_d) = asked(NEEDLE_FIRST, "Test Needle");
    let goblin = named(&other_t, &other_d, ChosenKind::CardName, "Test Goblin");
    assert_ne!(
        other_d.candidates.option(goblin),
        d.candidates.option(c),
        "the two games number the Goblin alike: the test shows nothing"
    );
    assert_eq!(loaded.find(&other_t, &other_s, &other_d), Some(goblin));
}

/// A creature type chosen as a land is played is saved as the type's name, and "another
/// creature type" as those words.
#[test]
fn a_chosen_creature_type_is_saved_by_name() {
    let (t, s, d) = asked(CREATURES_FIRST, "Test Cavern");
    assert_eq!(d.prompt.purpose, Purpose::CreatureType);
    for name in ["Goblin", "Elf", "Another creature type"] {
        let c = named(&t, &d, ChosenKind::CreatureType, name);
        let saved = Chosen::new(&t, &s, &d, c);
        let value = name.to_owned();
        assert_eq!(saved.what.as_ref().unwrap().act, Act::Chosen { value });
        assert_eq!(saved.find(&t, &s, &d), Some(c));
    }
}
