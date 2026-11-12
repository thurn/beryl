//! The rules library's functions called as no card calls them yet: each takes the rules'
//! parameters (a filter, a count, a player, an object, a position), and none assumes the
//! card it was first written for or the source of what is resolving.

use crate::actions::{manifest, tap, transform};
use crate::bitboard::Bitboard;
use crate::card::{Ability, FaceIx, ManaCost, Prompt, Purpose, Stop, Types};
use crate::decision::Candidates;
use crate::event::{self, Event, Reason};
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::resolution::Resolution;
use crate::state::{ObjectFlags, ObjectRef, StackEntry, TargetPlaces, TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{
    Choice, Decision, DecisionKind, GameTable, Opt, PlayerSet, Profile, Scratch, State, choices,
    coins, costs, library, tokens,
};
use beryl_card::effect::build::{YOU, scry};
use beryl_kernel::two_player::TwoPlayerSeat as _;

use super::events::{CARDS, DECK, INSECT, PRODIGY_CARDS, find, game, put};

/// An ability that may ask, as data that asks says of itself.
static ASKS: Ability = Ability::spell().effect(&scry(YOU, 1));

/// Does `act` as a step of an ability of `source` resolving, its questions answered from
/// `answers` in turn. Returns what it came to, and the question it stopped at if it did.
fn resolving<T>(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    source: ObjectRef,
    answers: &[u8],
    act: impl FnOnce(&mut Game, &mut Resolution) -> Result<T, Stop>,
) -> (Result<T, Stop>, Option<Decision>) {
    let entry = StackEntry {
        object: Some(source),
        controller: s.controller(source.slot),
        card: s.copiable(source.slot).record(t).0,
        ..StackEntry::default()
    };
    let mut resolution = Resolution::new(entry, &ASKS, TargetPlaces::NONE, None, answers);
    let result = act(&mut Game::new(s, t, &mut x.events), &mut resolution);
    crate::layers::refresh(s, t, &mut x.events).unwrap();
    (result, resolution.question.take())
}

/// The answer that picks `option` of `d`.
fn pick(d: &Decision, option: Opt) -> u8 {
    let Candidates::Options(options) = &d.candidates else {
        panic!("{d:?} offers no options");
    };
    let at = options.iter().position(|&o| o == option);
    u8::try_from(at.unwrap_or_else(|| panic!("{option:?} not among {d:?}"))).unwrap()
}

fn tapped(s: &State, slot: Slot) -> bool {
    s.object(slot).flags().has(ObjectFlags::TAPPED)
}

/// [CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "Only tapped permanents
/// can be untapped." Here an opponent of the ability's controller untaps a creature: neither
/// the lands nor the controller the helper was first written for.
#[test]
fn any_player_untaps_up_to_n_of_the_tapped_permanents_named() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let land = put(&t, &mut s, &mut x, p, "Test Land");
    let source = s.object_ref(land);
    let [mine, theirs] = [p, p.opponent()].map(|owner| put(&t, &mut s, &mut x, owner, "Test Bear"));
    let upright = put(&t, &mut s, &mut x, p, "Test Bear");
    for slot in [land, mine, theirs] {
        let permanent = s.object_ref(slot);
        tap::tap(&mut Game::new(&mut s, &t, &mut x.events), permanent).unwrap();
    }
    let creatures: Bitboard<Slot> = [mine, theirs, upright].into_iter().collect();
    let untap = |game: &mut Game, resolution: &mut Resolution| {
        tap::untap_up_to(game, resolution, p.opponent(), creatures, 2).map(|_| ())
    };

    let (paused, question) = resolving(&t, &mut s.clone(), &mut x, source, &[], untap);
    let d = question.expect("a question");
    assert_eq!(paused, Err(Stop::Pause));
    assert_eq!((d.player, d.kind), (p.opponent(), DecisionKind::ChooseUpTo));
    // The tapped creatures, whoever controls them: not the tapped land, not the untapped
    // creature.
    let offered: Vec<Opt> = (0..d.len() as u16)
        .map(|i| d.candidates.option(Choice(i)).unwrap())
        .collect();
    assert_eq!(offered.len(), 3);
    for option in [Opt::Done, Opt::Object(mine), Opt::Object(theirs)] {
        assert!(offered.contains(&option), "{option:?} not offered");
    }

    // One of the two, then done.
    let answers = [pick(&d, Opt::Object(mine)), pick(&d, Opt::Done)];
    let (done, question) = resolving(&t, &mut s, &mut x, source, &answers, untap);
    assert_eq!((done, question.is_none()), (Ok(()), true));
    assert!(!tapped(&s, mine) && !tapped(&s, upright));
    assert!(tapped(&s, theirs) && tapped(&s, land));
}

/// [CR 701.40a](https://yawgatog.com/resources/magic-rules/#R70140a): "To manifest a card,
/// turn it face down. ... Put that card onto the battlefield face down." Here a card from a
/// hand, for a player who does not own it: manifest dread manifests only its player's own
/// library's cards.
#[test]
fn any_card_is_manifested_for_any_player() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let card = s
        .in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Hand)))
        .next()
        .expect("a card in hand");
    let mut game = Game::new(&mut s, &t, &mut x.events);
    manifest::manifest(&mut game, card, p.opponent()).unwrap();
    crate::layers::refresh(&mut s, &t, &mut x.events).unwrap();
    let o = s.object(card);
    assert_eq!((o.zone(), o.face()), (Zone::Battlefield, FaceIx::DOWN));
    assert_eq!((o.owner(), s.controller(card)), (p, p.opponent()));
    assert_eq!(s.derived(card).types, Types::CREATURE);
    // Its owner knew it in their hand, and still does.
    assert!(s.who_knows(card).contains(p));
}

/// [CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a): "If a spell or ability
/// puts a double-faced card onto the battlefield 'transformed' or 'converted,' it enters the
/// battlefield with its back face up. If a player is instructed to put a card that isn't a
/// double-faced card onto the battlefield transformed or converted, that card stays in its
/// current zone." Neither permanent here is the source of anything resolving.
#[test]
fn any_permanent_is_exiled_and_only_a_double_faced_one_returns_transformed() {
    let t = GameTable::new(
        &PRODIGY_CARDS,
        [
            &[(1, "Test Prodigy"), (5, "Test Land")],
            &[(6, "Test Land")],
        ],
    )
    .unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let mut x = Scratch::new();
    let prodigy = put(&t, &mut s, &mut x, Seat::P0, "Test Prodigy");
    let land = put(&t, &mut s, &mut x, Seat::P1, "Test Land");
    let [was, single] = [prodigy, land].map(|slot| s.object_ref(slot));

    let mut game = Game::new(&mut s, &t, &mut x.events);
    transform::exile_and_return_transformed(&mut game, was).unwrap();
    transform::exile_and_return_transformed(&mut game, single).unwrap();
    // Neither is the object it was (CR 400.7).
    transform::exile_and_return_transformed(&mut game, was).unwrap();
    transform::exile_and_return_transformed(&mut game, single).unwrap();

    let o = s.object(prodigy);
    assert_eq!((o.zone(), o.face()), (Zone::Battlefield, FaceIx::BACK));
    assert_eq!(s.controller(prodigy), Seat::P0);
    assert!(!s.is_current(was));
    assert_eq!(s.object(land).zone(), Zone::Exile);
}

/// [CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017): "If an effect causes a
/// player to put a card into a library 'Nth from the top,' and that library has fewer than N
/// cards in it, the player puts that card on the bottom of that library." Here cards from a
/// graveyard, a hand and the library itself, where the cards put a spell and a permanent.
#[test]
fn any_object_is_put_into_its_owners_library_at_any_position() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let buried = find(&s, &t, p, "Test Bear");
    event::move_one(
        &mut s,
        &t,
        &mut x.events,
        buried,
        Zone::Graveyard,
        Reason::Effect,
    )
    .unwrap();
    let held = s
        .in_candidate_order(s.zone(ZoneKey::Of(p, Zone::Hand)))
        .next()
        .expect("a card in hand");
    let top = *s.library(p).last().unwrap();
    let [buried_card, held_card, top_card] = [buried, held, top].map(|slot| s.object_ref(slot));

    let mut game = Game::new(&mut s, &t, &mut x.events);
    library::put_into(&mut game, held_card, Place::UnderTop(2)).unwrap();
    library::put_into(&mut game, buried_card, Place::Bottom).unwrap();
    let cards = s.library(p);
    assert_eq!(cards[cards.len() - 3], held, "third from the top");
    assert_eq!((cards[0], *cards.last().unwrap()), (buried, top));
    assert!(!s.is_current(held_card) && !s.is_current(buried_card));

    // A card already in the library moves within it, and is the object it was.
    let mut game = Game::new(&mut s, &t, &mut x.events);
    library::put_into(&mut game, top_card, Place::Bottom).unwrap();
    assert_eq!(s.library(p)[..2], [top, buried]);
    assert!(s.is_current(top_card));
    // One that has changed zones since goes nowhere (CR 400.7).
    let before = s.library(p).len();
    let mut game = Game::new(&mut s, &t, &mut x.events);
    library::put_into(&mut game, held_card, Place::Top).unwrap();
    assert_eq!(s.library(p).len(), before);
}

/// [CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084): "If a creature is put onto
/// the battlefield attacking, its controller chooses which defending player, planeswalker a
/// defending player controls, or battle a defending player protects it's attacking as it
/// enters the battlefield". Here two tokens of another kind, and then defenders and a
/// controller for whom they cannot be attacking
/// ([CR 506.3b](https://yawgatog.com/resources/magic-rules/#R5063b),
/// [CR 506.3c](https://yawgatog.com/resources/magic-rules/#R5063c)).
#[test]
fn any_tokens_enter_attacking_one_of_the_defending_players_given() {
    let t = GameTable::new(&CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::DeclareAttackers);
    let mut x = Scratch::new();
    let land = put(&t, &mut s, &mut x, Seat::P0, "Test Land");
    let walker = put(&t, &mut s, &mut x, Seat::P1, "Test Walker");
    let source = s.object_ref(land);
    let insects = tokens::predefined(&t, &INSECT);
    let create = |controller: Seat, count: u8, defending: PlayerSet| {
        move |game: &mut Game, resolution: &mut Resolution| {
            tokens::create_attacking(game, resolution, controller, insects, count, defending)
        }
    };
    let flags = |s: &State, slot: Slot| {
        let flags = s.object(slot).flags();
        (
            flags.has(ObjectFlags::TAPPED),
            flags.has(ObjectFlags::ATTACKING),
        )
    };

    // The attacking player's, against the defending player: they choose what the two attack.
    let them = PlayerSet::only(Seat::P1);
    let (paused, question) = resolving(
        &t,
        &mut s.clone(),
        &mut x,
        source,
        &[],
        create(Seat::P0, 2, them),
    );
    let d = question.expect("a question");
    assert_eq!(paused.err(), Some(Stop::Pause));
    assert_eq!((d.player, d.kind), (Seat::P0, DecisionKind::AttackTarget));
    assert_eq!(d.about.count(), 2);
    let answers = [pick(&d, Opt::Defender(Some(walker)))];
    let (created, question) = resolving(
        &t,
        &mut s,
        &mut x,
        source,
        &answers,
        create(Seat::P0, 2, them),
    );
    let created = created.unwrap();
    assert!(question.is_none());
    assert_eq!(created.count(), 2);
    for token in s.in_candidate_order(created) {
        assert_eq!(flags(&s, token), (true, true));
        assert_eq!(s.object(token).combat(), Some(walker));
        assert_eq!(s.controller(token), Seat::P0);
    }

    // Against a player who is not defending, and under a player who is not attacking: tapped,
    // never attacking, and nothing to choose.
    let not_defending = create(Seat::P0, 1, PlayerSet::only(Seat::P0));
    let not_attacking = create(Seat::P1, 1, PlayerSet::only(Seat::P0));
    for (create, controller) in [(not_defending, Seat::P0), (not_attacking, Seat::P1)] {
        let (created, question) = resolving(&t, &mut s, &mut x, source, &[], create);
        let token = created.unwrap().sole().expect("one token");
        assert!(question.is_none());
        assert_eq!(flags(&s, token), (true, false));
        assert_eq!(s.controller(token), controller);
    }
}

/// [CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812): "[A player] may [do
/// something]. If [that player] [does, doesn't, or can't], [effect]." Here an opponent of
/// the ability's controller is offered a cost no card names, behind a question that is not
/// an "unless": they decline it, then pay it.
#[test]
fn any_player_pays_or_declines_any_mana_cost_an_effect_offers() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let source = s.object_ref(bear);
    let payer = p.opponent();
    let lands = [(); 2].map(|()| put(&t, &mut s, &mut x, payer, "Test Land"));
    let two = ManaCost::generic(2);
    let may = Prompt::new(Purpose::May);
    let offer = |cost| {
        move |game: &mut Game, resolution: &mut Resolution| {
            costs::may_pay(game, resolution, payer, cost, may)
        }
    };

    // A cost they cannot pay is not offered (CR 118.3).
    let (paid, question) = resolving(&t, &mut s, &mut x, source, &[], offer(ManaCost::generic(3)));
    assert_eq!((paid, question), (Ok(false), None));

    let (paused, question) = resolving(&t, &mut s.clone(), &mut x, source, &[], offer(two));
    let d = question.expect("a question");
    assert_eq!(paused, Err(Stop::Pause));
    assert_eq!((d.player, d.kind), (payer, DecisionKind::YesNo));
    assert_eq!(d.prompt, may.cost(two));

    x.events.drain();
    let no = [pick(&d, Opt::No)];
    let (paid, question) = resolving(&t, &mut s, &mut x, source, &no, offer(two));
    assert_eq!((paid, question), (Ok(false), None));
    assert!(lands.iter().all(|&land| !tapped(&s, land)));
    assert!(!(x.events.drain()).any(|e| matches!(e, Event::ResolutionPayment { .. })));

    // Two lands alike pay it one way, so the payment itself asks nothing.
    let yes = [pick(&d, Opt::Yes)];
    let (paid, question) = resolving(&t, &mut s, &mut x, source, &yes, offer(two));
    assert_eq!((paid, question), (Ok(true), None));
    assert!(lands.iter().all(|&land| tapped(&s, land)));
    assert!(
        (x.events.drain())
            .any(|e| matches!(e, Event::ResolutionPayment { player, .. } if player == payer))
    );
}

/// [CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052): "Only the player who flips
/// the coin wins or loses the flip; no other players are involved." Here a player flips for
/// an effect of a permanent they do not control, and every flip is told with its source.
#[test]
fn any_player_flips_a_coin_for_any_source() {
    let (t, mut s, p) = game();
    let mut x = Scratch::recording();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let source = s.object_ref(bear);
    let flipper = p.opponent();
    x.events.drain();
    let mut results = [0, 0];
    for _ in 0..32 {
        let mut game = Game::new(&mut s, &t, &mut x.events);
        let won = coins::flip(&mut game, flipper, source).unwrap();
        results[usize::from(won)] += 1;
        let told: Vec<Event> = x.events.drain().collect();
        let flip = Event::CoinFlip {
            player: flipper,
            won,
            source,
        };
        assert_eq!(told, [flip]);
    }
    assert!(results[0] > 0 && results[1] > 0, "{results:?}");
}

/// A prompt is asked in its purpose's shape: a question that cannot answer it is refused
/// where it is written, before any player sees it.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "is not answered yes or no")]
fn a_prompt_asked_through_another_shapes_function_is_refused() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let source = s.object_ref(bear);
    let search = Prompt::new(Purpose::Search);
    let _ = resolving(&t, &mut s, &mut x, source, &[], |game, resolution| {
        choices::yes_no(game, resolution, p, search, Bitboard::EMPTY)
    });
}
