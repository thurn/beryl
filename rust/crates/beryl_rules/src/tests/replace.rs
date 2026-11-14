//! Replacement effects applied to a proposal before it changes the state ([CR 616.1]): "If
//! two or more replacement and/or prevention effects are attempting to modify the way an event
//! affects an object or player, the affected object's controller (or its owner if it has no
//! controller) or the affected player chooses one to apply".
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::effect::build::{win_game, you};

use crate::actions::destroy::destroy;
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, Players, Replaced, Replacement, Rewrite, Types};
use crate::continuous;
use crate::drawing;
use crate::event::{Reason, Sink, TokenEntry};
use crate::filter::Filter;
use crate::game::Game;
use crate::ids::Seat;
use crate::replacement::Unreplaced;
use crate::state::{Continuous, Duration, MoveReplacement, TurnStep, Zone};
use crate::table::TableError;
use crate::{CardTable, GameTable, Outcome, Profile, Scratch, State, layers, tokens, zones};

use super::LAND;
use super::events::{put, with_game};

/// "If a card or token would be put into a graveyard from anywhere, exile it instead."
static REST: CardDef = CardDef {
    name: "Test Rest",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::put_into(
            &Filter::Any,
            Zone::Graveyard,
            Zone::Exile,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

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

/// "If an opponent would draw a card, you win the game instead."
static WATCH: CardDef = CardDef {
    name: "Test Watch",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[
            Ability::replacing(&Replacement::draws(Players::EachOpponent)).effect(&win_game(you())),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A move replaced by a rewrite of entering, which the engine has no code for.
static TANGLE: CardDef = CardDef {
    name: "Test Tangle",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement {
            replaces: Replaced::PutInto(&Filter::Any, Zone::Graveyard),
            rewrite: Rewrite::EntersTapped,
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "If a card would be put into a graveyard, put it into a graveyard instead": no change.
static ECHO: CardDef = CardDef {
    name: "Test Echo",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::put_into(
            &Filter::Any,
            Zone::Graveyard,
            Zone::Graveyard,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &REST, &BEAR, &WATCH, &TANGLE, &ECHO]);

/// A game in the first player's main phase, each deck holding two Rests, three Bears and a
/// Watch.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (2, "Test Rest"),
        (3, "Test Bear"),
        (1, "Test Watch"),
        (9, "Test Land"),
    ];
    let t = GameTable::new(&CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    (t, s)
}

/// A token that would be put into a graveyard is exiled, as a card is: "a card or token".
#[test]
fn a_token_that_would_die_is_exiled() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let p = Seat::P0;
    put(&t, &mut s, &mut x, p, "Test Rest");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let values = s.copiable(bear);
    let mut token = None;
    with_game(&t, &mut s, &mut x, bear, |game, resolution| {
        let how = TokenEntry::default();
        let made = tokens::create(game, resolution.controller(), values, 1, how).unwrap();
        token = made.sole();
        let token = game.state.object_ref(token.unwrap());
        destroy(game, token).unwrap();
    });
    assert_eq!(s.object(token.unwrap()).zone(), Zone::Exile);
}

/// The replacements are found before any object of a batch moves, so Rest destroyed with a
/// creature exiles both, itself among them; with Rest gone, a creature goes to the graveyard.
#[test]
fn a_batch_is_replaced_as_it_was_before_it_moved() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let p = Seat::P0;
    let rest = put(&t, &mut s, &mut x, p, "Test Rest");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let other = put(&t, &mut s, &mut x, Seat::P1, "Test Bear");
    let batch = Bitboard::single(rest) | Bitboard::single(bear);
    let mut game = Game::new(&mut s, &t, &mut x.events);
    zones::move_all(&mut game, batch, Zone::Graveyard, Reason::Destroy).unwrap();
    assert_eq!(s.object(rest).zone(), Zone::Exile);
    assert_eq!(s.object(bear).zone(), Zone::Exile);
    let mut game = Game::new(&mut s, &t, &mut x.events);
    let other = Bitboard::single(other);
    zones::move_all(&mut game, other, Zone::Graveyard, Reason::Destroy).unwrap();
    assert_eq!(s.object(other.sole().unwrap()).zone(), Zone::Graveyard);
}

/// Two Rests and an "exile it instead" effect on the creature all put it into exile: they
/// are one candidate, applied together, and the creature is exiled once.
#[test]
fn replacements_putting_an_object_into_the_same_zone_are_one() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let p = Seat::P0;
    put(&t, &mut s, &mut x, p, "Test Rest");
    let rest = put(&t, &mut s, &mut x, p, "Test Rest");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let bear_ref = s.object_ref(bear);
    with_game(&t, &mut s, &mut x, rest, |game, resolution| {
        let exile = Continuous::Move(bear_ref, MoveReplacement::ExileInsteadOfGraveyard);
        continuous::start(game, resolution, exile, Duration::EndOfTurn).unwrap();
        destroy(game, bear_ref).unwrap();
    });
    assert_eq!(s.object(bear).zone(), Zone::Exile);
}

/// A replacement of an opponent's draws replaces the opponent's draws, not its controller's.
#[test]
fn a_replacement_of_an_opponents_draws() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, Seat::P0, "Test Watch");
    drawing::draw(&mut Game::new(&mut s, &t, &mut x.events), Seat::P0, 1).unwrap();
    assert_eq!(s.outcome(), None);
    drawing::draw(&mut Game::new(&mut s, &t, &mut x.events), Seat::P1, 1).unwrap();
    assert_eq!(s.outcome(), Some(Outcome::Win(Seat::P0)));
}

/// The card table refuses a replacement of a move that the engine has no code for: a rewrite
/// of entering, or one that puts the object where it was going anyway.
#[test]
fn a_table_refuses_a_replacement_of_a_move_the_engine_cannot_do() {
    for card in ["Test Tangle", "Test Echo"] {
        let table = GameTable::new(&CARDS, [&[(1, card)], &[(1, "Test Land")]]);
        let unreplaced = Unreplaced::Rewrite;
        assert_eq!(
            table.err(),
            Some(TableError::Replacement { card, unreplaced })
        );
    }
}
