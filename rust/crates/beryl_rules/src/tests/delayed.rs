//! Delayed triggered abilities that carry an object ([CR 603.7c]): "A delayed triggered
//! ability that refers to a particular object still affects it even if the object changes
//! characteristics. However, if that object is no longer in the zone it's expected to be in at
//! the time the delayed triggered ability resolves, the ability won't affect it." A test-only
//! Flickerwisp exiles a permanent as it enters and returns that card to the battlefield under
//! its owner's control at the beginning of the next end step.
//!
//! [CR 603.7c]: https://yawgatog.com/resources/magic-rules/#R6037c

use beryl_card::effect::build::{
    carried_object, delay_with, exile, return_to_battlefield, sequence, target, those,
};

use super::LAND;
use super::cast::{BEAR, Game};
use super::events::with_game;
use crate::card::{Ability, CardDef, Face, Players, TargetSpec, Trigger, Types};
use crate::event::{self, Reason, TokenEntry};
use crate::filter::Filter;
use crate::ids::Slot;
use crate::resolution::Resolution;
use crate::state::{Duration, TurnStep, Zone};
use crate::{CardTable, Choice, DecisionKind, Decklist, Profile, continuous, tokens, zones};

/// "When this creature enters, exile another target permanent. Return that card to the
/// battlefield under its owner's control at the beginning of the next end step."
static FLICKERWISP: CardDef = CardDef {
    name: "Test Flickerwisp",
    tokens: &[],
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(1),
        abilities: &[
            Ability::triggered(&Trigger::ENTERS)
                .targeting(&[TargetSpec::permanent(Filter::Any).other()])
                .effect(&sequence(&[exile(target()), delay_with(1, those())])),
            Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
                .effect(&return_to_battlefield(carried_object())),
        ],
        ..Face::EMPTY
    }],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BEAR, &FLICKERWISP]);
const DECK: &Decklist = &[(30, "Test Land"), (5, "Test Bear"), (5, "Test Flickerwisp")];

/// Puts a Test Flickerwisp of the starting player's onto the battlefield and resolves its
/// enters ability, targeting `permanent`.
fn flicker(g: &mut Game, permanent: Slot) {
    let wisp = g.give(g.me, "Test Flickerwisp", Zone::Library);
    event::move_one(
        &mut g.s,
        &g.t,
        &mut g.x.events,
        wisp,
        Zone::Battlefield,
        Reason::Effect,
    )
    .unwrap();
    // A target that is the only one legal is chosen without asking.
    if g.decide().kind == DecisionKind::Target {
        g.pick_object(permanent);
    }
    while !g.s.stack().is_empty() || !g.s.triggers().is_empty() {
        g.choose(Choice(0));
    }
    // State-based actions are checked before the next decision.
    assert_eq!(g.decide().kind, DecisionKind::Priority);
    assert_eq!(g.s.delayed().len(), 1, "the return waits for the end step");
}

/// Changes the game as an ability of `source` does, then brings the layers up to date.
fn with_game_at(
    g: &mut Game,
    source: Slot,
    body: impl FnOnce(&mut crate::game::Game, &mut Resolution),
) {
    crate::layers::refresh(&mut g.s, &g.t, &mut g.x.events).unwrap();
    with_game(&g.t, &mut g.s, &mut g.x, source, body);
    crate::layers::refresh(&mut g.s, &g.t, &mut g.x.events).unwrap();
}

/// A game in the starting player's precombat main phase. Each player has a land in hand,
/// so every main phase of theirs is a decision and the game stops there.
fn game() -> Game {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    for player in [g.me, g.them] {
        g.give(player, "Test Land", Zone::Hand);
    }
    g
}

/// Passes, declares no attackers and plays no land until the turn after this one begins,
/// through this turn's end step.
fn to_next_turn(g: &mut Game) {
    let me = g.s.turn().active();
    while g.s.turn().active() == me {
        g.decide();
        g.choose(Choice(0));
    }
}

#[test]
fn an_exiled_card_returns_at_the_next_end_step_untapped_under_its_owners_control() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    let thief = g.give(me, "Test Bear", Zone::Battlefield);
    let bear = g.give(them, "Test Bear", Zone::Battlefield);
    let stolen = g.s.object_ref(bear);
    with_game_at(&mut g, thief, |game, resolution| {
        continuous::gain_control(game, resolution, stolen, Duration::Indefinite).unwrap();
    });
    g.s.board().set_tapped(bear, true);
    assert_eq!(g.s.controller(bear), me);
    flicker(&mut g, bear);
    assert_eq!(g.s.object(bear).zone(), Zone::Exile);
    // CR 400.7: the card in exile is a new object, which is the one the ability carries.
    let exiled = g.s.object_ref(bear);
    assert_ne!(exiled, stolen);
    assert_eq!(g.s.delayed()[0].info.object, Some(exiled));
    to_next_turn(&mut g);
    assert!(g.s.delayed().is_empty());
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
    assert_ne!(g.s.object_ref(bear), exiled, "it returns as a new object");
    // CR 110.2a: under its owner's control, and the effect that stole it ended with the
    // object it applied to.
    assert_eq!(g.s.controller(bear), them);
    assert!(!g.s.object(bear).is_tapped());
}

#[test]
fn an_exiled_token_has_ceased_to_exist_and_does_not_return() {
    let mut g = game();
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    let values = g.s.copiable(bear);
    let mut token = None;
    with_game_at(&mut g, bear, |game, resolution| {
        let made = tokens::create(
            game,
            resolution.controller(),
            values,
            1,
            TokenEntry::default(),
        );
        token = made.unwrap().sole();
    });
    let token = token.expect("a token copy");
    flicker(&mut g, token);
    // CR 111.7: "A token that's in a zone other than the battlefield ceases to exist."
    assert_eq!(g.s.object(token).zone(), Zone::Nowhere);
    let before = g.s.battlefield().len();
    to_next_turn(&mut g);
    assert!(g.s.delayed().is_empty());
    assert_eq!(g.s.battlefield().len(), before);
    assert!(
        !g.s.battlefield()
            .iter()
            .any(|&slot| g.s.object(slot).is_token())
    );
}

#[test]
fn a_card_that_left_exile_before_the_end_step_stays_where_it_went() {
    let mut g = game();
    let bear = g.give(g.them, "Test Bear", Zone::Battlefield);
    flicker(&mut g, bear);
    let exiled = g.s.object_ref(bear);
    let wisp =
        *g.s.battlefield()
            .iter()
            .find(|&&slot| g.s.definition_name(&g.t, slot) == "Test Flickerwisp")
            .unwrap();
    with_game_at(&mut g, wisp, |game, _| {
        zones::move_to(game, exiled, Zone::Graveyard, Reason::Effect).unwrap();
    });
    assert_eq!(g.s.object(bear).zone(), Zone::Graveyard);
    to_next_turn(&mut g);
    assert!(g.s.delayed().is_empty());
    // CR 603.7c: it is no longer in the zone it was expected to be in.
    assert_eq!(g.s.object(bear).zone(), Zone::Graveyard);
}
