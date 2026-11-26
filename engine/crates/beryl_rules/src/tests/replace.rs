//! Replacement effects applied to a proposal before it changes the state ([CR 616.1]): "If
//! two or more replacement and/or prevention effects are attempting to modify the way an event
//! affects an object or player, the affected object's controller (or its owner if it has no
//! controller) or the affected player chooses one to apply".
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161

use beryl_card::effect::build::{create_tokens, win_game, you};

use crate::actions::destroy::destroy;
use crate::actions::manifest::manifest;
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, Face, Players, Purpose, Replaced, Replacement, Rewrite, Types,
};
use crate::continuous;
use crate::decision::{Choice, DecisionKind, Opt};
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::replacement::event::{Reason, Sink, TokenEntry};
use crate::state::{Continuous, Duration, LayerChange, MoveReplacement, TurnStep, Zone};
use crate::table::TableError;
use crate::zones::drawing;
use crate::{
    CardTable, GameTable, Outcome, Profile, Replay, Scratch, State, apply, continuous::layers,
    zones, zones::library, zones::tokens,
};
use beryl_kernel::format::Format;
use beryl_kernel::replacement::Unreplaced;

use super::events::{put, with_game};
use super::{LAND, decide};

/// "If a card or token would be put into a graveyard from anywhere, exile it instead."
static REST: CardDef = CardDef {
    name: "Test Rest",
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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
    shape: beryl_card::definition::Shape::Single,
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

/// "If a nontoken creature would enter and it wasn't cast, exile it instead."
static PRIEST: CardDef = CardDef {
    name: "Test Priest",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[Ability::replacing(&Replacement::enters_not_cast(
            &Filter::All(&[Filter::Not(&Filter::Token), Filter::Types(Types::CREATURE)]),
            Zone::Exile,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A 1/1 token created by [`MAKE_TOKEN`].
static TOKEN: CardDef = CardDef {
    name: "Test Token",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "If an effect would create one or more tokens under your control, it creates twice that many
/// of those tokens instead."
static DOUBLE_TOKENS: CardDef = CardDef {
    name: "Test Double Tokens",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::tokens_created(
            Players::You,
            Rewrite::Multiply(2),
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "If an effect would create one or more tokens under your control, it creates those tokens
/// plus an additional token instead."
static PLUS_ONE_TOKEN: CardDef = CardDef {
    name: "Test Plus One Token",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::replacing(&Replacement::tokens_created(
            Players::You,
            Rewrite::Plus(1),
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Free sorcery that creates one [`TOKEN`].
static MAKE_TOKEN: CardDef = CardDef {
    name: "Test Make Token",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&create_tokens(you(), &TOKEN, 1))],
        ..Face::EMPTY
    }],
    tokens: &[&TOKEN],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &REST,
    &BEAR,
    &WATCH,
    &TANGLE,
    &ECHO,
    &PRIEST,
    &DOUBLE_TOKENS,
    &PLUS_ONE_TOKEN,
    &MAKE_TOKEN,
]);

/// A game in the first player's main phase, each deck holding two Rests, three Bears, a Watch
/// and a Priest.
fn game() -> (GameTable, State) {
    let deck: &[(u8, &str)] = &[
        (2, "Test Rest"),
        (3, "Test Bear"),
        (1, "Test Watch"),
        (1, "Test Priest"),
        (9, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
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
        destroy(game, game.state().permanent(token).unwrap()).unwrap();
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
        destroy(game, game.state().permanent(bear_ref).unwrap()).unwrap();
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

/// The player who now controls a draw replacement is the one whose opponents it affects
/// ([CR 109.5]): "The words “you” and “your” on an object refer to the object’s controller".
///
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
#[test]
fn a_draw_replacement_reads_the_current_controller() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let watch = put(&t, &mut s, &mut x, Seat::P0, "Test Watch");
    let (card, _) = s.definition(watch).record(&t);
    let control = Continuous::Layer(
        s.permanent_at(watch).unwrap(),
        LayerChange::Control(Seat::P1),
    );
    s.start_effect(card, control, Duration::EndOfTurn).unwrap();

    drawing::draw(&mut Game::new(&mut s, &t, &mut x.events), Seat::P0, 1).unwrap();
    assert_eq!(s.outcome(), Some(Outcome::Win(Seat::P1)));
}

/// The card table refuses a replacement of a move that the engine has no code for: a rewrite
/// of entering, or one that puts the object where it was going anyway.
#[test]
fn a_table_refuses_a_replacement_of_a_move_the_engine_cannot_do() {
    for card in ["Test Tangle", "Test Echo"] {
        let table = GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            [&[(1, card)], &[(1, "Test Land")]],
        );
        let unreplaced = Unreplaced::Rewrite;
        assert_eq!(
            table.err(),
            Some(TableError::Replacement { card, unreplaced })
        );
    }
}

/// A land manifested while a Priest is on the battlefield would be a face-down 2/2 creature
/// there, which wasn't cast, so it is exiled instead ([CR 614.12]): "check the characteristics
/// of the permanent as it would exist on the battlefield". It is no face-down permanent, so
/// both players saw it go to exile ([CR 400.2]): "Public zones are zones in which all players
/// can see the cards' faces".
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
/// [CR 400.2]: https://yawgatog.com/resources/magic-rules/#R4002
#[test]
fn a_card_manifested_under_a_priest_is_exiled_face_up() {
    let (t, mut s) = game();
    let mut x = Scratch::new();
    let p = Seat::P0;
    let priest = put(&t, &mut s, &mut x, p, "Test Priest");
    let card = library::top(&s, p, 1).sole().unwrap();
    with_game(&t, &mut s, &mut x, priest, |game, _| {
        manifest(game, card, p).unwrap();
    });
    assert_eq!(s.object(card).zone(), Zone::Exile);
    assert_eq!(s.who_knows(card), s.players());
}

/// With both a multiplier (`Multiply(2)`) and an addition (`Plus(1)`), the player who would
/// control the tokens chooses which applies first (CR 616.1), giving 3 (`1 * 2 + 1`) or 4
/// (`(1 + 1) * 2`).
#[test]
fn multiply_and_plus_token_replacements_ask_and_give_both_answers() {
    let deck: &[(u8, &str)] = &[
        (1, "Test Double Tokens"),
        (1, "Test Plus One Token"),
        (1, "Test Make Token"),
        (9, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    let doubler = put(&t, &mut s, &mut x, Seat::P0, "Test Double Tokens");
    let adder = put(&t, &mut s, &mut x, Seat::P0, "Test Plus One Token");
    let spell = s
        .library(Seat::P0)
        .iter()
        .copied()
        .find(|&slot| t.def(s.definition(slot).record(&t).0).name == "Test Make Token")
        .unwrap();
    s.move_object(&mut Sink::off(), spell, Zone::Hand, crate::zone::Place::Top)
        .unwrap();
    layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();

    // Cast the free sorcery and let it resolve up to the replacement order choice.
    let d = decide(&t, &mut s);
    let cast = (0..d.len() as u16)
        .find(|&i| {
            matches!(
                d.candidates.option(Choice(i)),
                Some(Opt::Cast { slot, .. }) if slot == spell
            )
        })
        .unwrap();
    apply(&mut s, &t, &mut x, &mut Replay, Choice(cast));
    let d = loop {
        let d = decide(&t, &mut s);
        if d.kind != DecisionKind::Priority {
            break d;
        }
        let pass = (0..d.len() as u16)
            .find(|&i| matches!(d.candidates.option(Choice(i)), Some(Opt::Pass)))
            .unwrap();
        apply(&mut s, &t, &mut x, &mut Replay, Choice(pass));
    };
    assert_eq!(
        (d.kind, d.prompt.purpose, d.player, d.len()),
        (DecisionKind::ChooseObject, Purpose::Replace, Seat::P0, 2)
    );

    let count_tokens = |s: &State| {
        s.battlefield()
            .iter()
            .filter(|&&slot| t.def(s.definition(slot).record(&t).0).name == "Test Token")
            .count()
    };

    // Doubler first: 1 * 2 + 1 = 3.
    let mut s_mul = s;
    let pick_mul = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::Object(doubler)))
        .unwrap();
    apply(&mut s_mul, &t, &mut x, &mut Replay, Choice(pick_mul));
    assert_eq!(count_tokens(&s_mul), 3);

    // Adder first: (1 + 1) * 2 = 4.
    let mut s_add = s;
    let pick_add = (0..d.len() as u16)
        .find(|&i| d.candidates.option(Choice(i)) == Some(Opt::Object(adder)))
        .unwrap();
    apply(&mut s_add, &t, &mut x, &mut Replay, Choice(pick_add));
    assert_eq!(count_tokens(&s_add), 4);
}

/// Doubling 200 tokens overflows `u8` and returns [`Full`] in `Small` instead of saturating.
#[test]
fn doubling_two_hundred_tokens_is_full_in_small() {
    let deck: &[(u8, &str)] = &[
        (1, "Test Double Tokens"),
        (1, "Test Bear"),
        (9, "Test Land"),
    ];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, Seat::P0, "Test Double Tokens");
    let bear = put(&t, &mut s, &mut x, Seat::P0, "Test Bear");
    let values = s.copiable(bear);
    let mut result = Ok(Bitboard::EMPTY);
    with_game(&t, &mut s, &mut x, bear, |game, _| {
        result = tokens::create(game, Seat::P0, values, 200, TokenEntry::default());
    });
    assert_eq!(result, Err(Full));
}
