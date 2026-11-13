//! The mode and number shapes' prompts answered by clicks alone: the ways to use a
//! card clicked at a priority, the value of X, a card offered to cast, and the opening's
//! choices. Each test holds the prompt's state after each click to a text written by hand
//! from what the screen shows, and checks that the clicks end where the picks a UI
//! holding the prompt's state itself would send end.

#[path = "common/play.rs"]
mod play;

use beryl_cards::names::{
    COUNTERSPELL, ELITE_SPELLBINDER, ISLAND, MALCOLM_ALLURING_SCOUNDREL, MANA_CRYPT, PLAINS,
    SNAPCASTER_MAGE, TEFERI_TIME_RAVELER, THIEVING_SKYDIVER,
};
use beryl_engine::card::Purpose;
use beryl_engine::state::CounterKind;
use beryl_engine::{Seat, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::{ButtonId, Click};
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::{StartGame, StopsSetting};
use play::{Play, cards};

/// The button of the answer at `index` of the open decision.
const fn option(index: u16) -> ButtonId {
    ButtonId::Option { index }
}

/// The choices of the log from entry `from` on: the kind of decision each answered and
/// whether it continues the one before.
fn choices(play: &Play, from: usize) -> Vec<(SavedKind, bool)> {
    (play.game.replay().log[from..].iter())
        .filter_map(|entry| match entry {
            Entry::Choice(chosen) => Some((chosen.what.as_ref()?.kind, chosen.continuation)),
            _ => None,
        })
        .collect()
}

/// The index of the candidate of the open decision that `wanted` names.
fn index(play: &Play, wanted: impl Fn(&Candidate) -> bool) -> u16 {
    let at = (play.decision().candidates.iter()).position(|c| wanted(&c.option));
    at.expect("such a candidate") as u16
}

/// Whether a candidate casts with these of its face's optional costs.
fn casts(with: &'static [u8]) -> impl Fn(&Candidate) -> bool {
    move |c| matches!(c, Candidate::Cast { costs, .. } if costs == with)
}

/// The human's main phase with a Thieving Skydiver to cast, kicked or not, four Islands, and
/// an opponent's Mana Crypt for its kicker to steal.
const SKYDIVER: Scenario = Scenario::new("ways and x")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIEVING_SKYDIVER])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .permanents(P1, &[Permanent::new(MANA_CRYPT)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A click at a priority on a card with several ways to use it opens the choice among them
/// over the priority, in the session: a button for each way, and one that goes back.
#[test]
fn a_card_with_several_ways_opens_the_choice_among_them() {
    let mut play = Play::scenario(&SKYDIVER);
    let diver = play.named(THIEVING_SKYDIVER, ZoneName::Hand, Side::You)[0];
    let (plain, kicked) = (index(&play, casts(&[])), index(&play, casts(&[0])));
    let priority = play.prompt();
    let ways = format!(
        "mode act: Choose One\n\
         about #{diver}\n\
         {plain}[Cast Thieving Skydiver] {kicked}[Cast Thieving Skydiver with kicker {{X}}] \
         decline[Cancel]\n"
    );
    assert_eq!(play.on(diver).as_deref(), Some(ways.as_str()));
    // The choice takes every click while it is open: the card again and a button it does not
    // have change nothing.
    assert_eq!(play.on(diver).as_deref(), Some(ways.as_str()));
    assert_eq!(play.press(ButtonId::PassOn).as_deref(), Some(ways.as_str()));
    // The game holds it: a view drawn again shows the choice as the click left it.
    let redrawn = play.game.set_stops(StopsSetting::SMART).unwrap();
    play.view = redrawn;
    assert_eq!(play.prompt(), ways);
    // Its cancel button goes back to the priority.
    assert_eq!(
        play.press(ButtonId::Decline).as_deref(),
        Some(priority.as_str())
    );
    // A way's button answers the priority with it, as picking that candidate does.
    let mut picked = Play::scenario(&SKYDIVER);
    picked.view = picked.game.set_stops(StopsSetting::SMART).unwrap();
    picked.choose(casts(&[]));
    play.on(diver);
    assert_eq!(play.press(option(plain)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        play.named(THIEVING_SKYDIVER, ZoneName::Battlefield, Side::You)
            .len(),
        1
    );
}

/// The human's main phase with a Teferi, Time Raveler at 1 loyalty, whose −3 it cannot pay,
/// and an opponent's creature for the −3 to return.
const WALKER: Scenario = Scenario::new("walker low")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[Permanent::new(TEFERI_TIME_RAVELER).counters(CounterKind::Loyalty, 1)],
    )
    .permanents(P1, &[Permanent::new(SNAPCASTER_MAGE)])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A click on a planeswalker whose loyalty pays only its +1 opens the choice among its
/// abilities all the same, the −3 it cannot activate shown in the card's order and never
/// enabled, as Arena's "Choose One" dims it (`2VW6ZzmS8U4 6:24.5`). A press of the −3 does
/// nothing; the +1 activates.
#[test]
fn a_planeswalker_shows_the_abilities_it_cannot_use() {
    let mut play = Play::scenario(&WALKER);
    let teferi = play.named(TEFERI_TIME_RAVELER, ZoneName::Battlefield, Side::You)[0];
    let plus = index(&play, |c| matches!(c, Candidate::Activate { .. }));
    let ways = format!(
        "mode act: Choose One\n\
         about #{teferi}\n\
         {plus}[Teferi, Time Raveler: +1] ability1(Teferi, Time Raveler: \u{2212}3) \
         decline[Cancel]\n"
    );
    assert_eq!(play.on(teferi).as_deref(), Some(ways.as_str()));
    assert_eq!(
        play.press(ButtonId::Ability { nth: 1 }).as_deref(),
        Some(ways.as_str())
    );
    // The +1 answers the priority with its activation, as picking that candidate does.
    let mut picked = Play::scenario(&WALKER);
    picked.choose(|c| matches!(c, Candidate::Activate { .. }));
    assert_eq!(play.press(option(plus)), None);
    assert_eq!(play.game.replay().log, picked.game.replay().log);
}

/// The value of X opens at the least the decision offers, says the cost each value makes and
/// marks what paying it would tap. A click on a number sets it and the button pays. Its
/// answer continues the cast, so one undo takes back both.
#[test]
fn an_x_is_set_and_then_paid() {
    let mut play = Play::scenario(&SKYDIVER);
    let diver = play.named(THIEVING_SKYDIVER, ZoneName::Hand, Side::You)[0];
    let islands = play.named(ISLAND, ZoneName::Battlefield, Side::You);
    let kicked = index(&play, casts(&[0]));
    let from = play.game.replay().log.len();
    play.on(diver);
    assert_eq!(play.press(option(kicked)), None, "the kicked cast is begun");
    // Two Islands pay for the Skydiver, so X is one or two. The engine's payment of an X of
    // one leaves an Island: which, the decision's candidate says.
    let three = play.decision().candidates[0].payments[0].taps.clone();
    assert_eq!(three.len(), 3);
    let one = format!(
        "number x: Select a value for X.\n\
         number 1 of 1 2 ({{2}}{{U}})\n\
         suggested {}\n\
         confirm[Pay | X = 1]*\n",
        cards(&three),
    );
    assert_eq!(play.prompt(), one);
    let two = format!(
        "number x: Select a value for X.\n\
         number 2 of 1 2 ({{3}}{{U}})\n\
         suggested {}\n\
         confirm[Pay | X = 2]*\n",
        cards(&islands),
    );
    let set = |n: u8| Click::Number { n, on: None };
    assert_eq!(play.click(set(2)).as_deref(), Some(two.as_str()));
    // A number the decision does not offer, and the one already set, change nothing.
    assert_eq!(play.click(set(3)).as_deref(), Some(two.as_str()));
    assert_eq!(play.click(set(2)).as_deref(), Some(two.as_str()));
    assert_eq!(play.click(set(1)).as_deref(), Some(one.as_str()));
    assert_eq!(play.click(set(2)).as_deref(), Some(two.as_str()));

    // The button pays: the Skydiver enters kicked for two and takes the Mana Crypt, as the
    // cast and its X picked one after the other do.
    let mut picked = Play::scenario(&SKYDIVER);
    picked.choose(casts(&[0]));
    picked.follow(|c| *c == Candidate::Number { n: 2 });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(
        choices(&play, from)[..2],
        [(SavedKind::Priority, false), (SavedKind::X, true)]
    );
    assert_eq!(
        play.named(MANA_CRYPT, ZoneName::Battlefield, Side::You)
            .len(),
        1
    );
    // One undo takes back the cast with its X.
    play.view = play.game.undo().unwrap();
    assert_eq!(
        play.named(THIEVING_SKYDIVER, ZoneName::Hand, Side::You),
        [diver]
    );
    assert!(play.decision().prompt.is(Purpose::Priority));
}

/// The human's turn with Malcolm, Alluring Scoundrel, a hit from casting what it discards,
/// and an Elite Spellbinder to draw.
const MALCOLM: Scenario = Scenario::new("cast offer")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[Permanent::new(MALCOLM_ALLURING_SCOUNDREL).counters(CounterKind::Chorus, 3)],
    )
    .library(P0, &[ELITE_SPELLBINDER, PLAINS, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Attacks with Malcolm, to the offer of the Elite Spellbinder its trigger drew and
/// discarded.
fn offered() -> Play {
    let mut play = Play::scenario(&MALCOLM);
    while !play.decision().prompt.is(Purpose::CastOffer) {
        let attacks = |c: &Candidate| matches!(c, Candidate::Attack { .. });
        match play.decision().kind {
            Kind::Attacker
                if play
                    .decision()
                    .candidates
                    .iter()
                    .any(|c| attacks(&c.option)) =>
            {
                play.choose(attacks);
            }
            Kind::Attacker => play.choose(|c| *c == Candidate::Done),
            _ => play.choose(|c| *c == Candidate::Pass),
        }
    }
    play
}

/// A card offered to cast is two buttons about it: the pass declines, and the cast is put
/// forward.
#[test]
fn a_card_offered_to_cast_is_cast_or_declined() {
    let mut play = offered();
    let binder = play.named(ELITE_SPELLBINDER, ZoneName::Graveyard, Side::You)[0];
    let (pass, cast) = (
        index(&play, |c| *c == Candidate::Pass),
        index(&play, casts(&[])),
    );
    let open = format!(
        "mode cast-offer: Cast that card?\n\
         about #{binder}\n\
         {pass}[Decline] {cast}[Cast]*\n"
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(play.on(binder).as_deref(), Some(open.as_str()));
    let mut picked = offered();
    picked.choose(casts(&[]));
    assert_eq!(play.press(option(cast)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert!(
        play.named(ELITE_SPELLBINDER, ZoneName::Graveyard, Side::You)
            .is_empty(),
        "it was cast"
    );

    // Declining leaves it in the graveyard, as the pass does.
    let mut play = offered();
    let mut picked = offered();
    picked.choose(|c| *c == Candidate::Pass);
    assert_eq!(play.press(option(pass)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        play.named(ELITE_SPELLBINDER, ZoneName::Graveyard, Side::You)
            .len(),
        1
    );
}

/// A game of Islands and Counterspells from `seed`, at the human's first decision.
fn dealt(seed: u64) -> Play {
    let deck = vec![(30, ISLAND.to_owned()), (30, COUNTERSPELL.to_owned())];
    Play::seeded(&StartGame {
        seed,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::SMART,
    })
}

/// The opening's two choices are a button for each answer: who plays first, asked of the
/// winner of the flip, and whether to keep the hand, over its cards.
#[test]
fn the_opening_is_played_by_its_buttons() {
    // The first seed whose flip the human wins.
    let seed = (0..32)
        .find(|&seed| dealt(seed).decision().prompt.is(Purpose::PlayOrDraw))
        .expect("a flip the human wins");
    let mut play = dealt(seed);
    assert_eq!(
        play.prompt(),
        "mode play-or-draw: Play or draw?\n\
         0[Play]* 1[Draw]\n"
    );
    let mut picked = dealt(seed);
    picked.choose(|c| *c == Candidate::Play);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());

    let hand: Vec<ObjectId> = (play.view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
        .map(|o| o.slot)
        .collect();
    assert_eq!(hand.len(), 7);
    assert_eq!(play.decision().prompt.about.len(), 7);
    assert_eq!(
        play.prompt(),
        format!(
            "mode mulligan: Keep or mulligan?\n\
             about {}\n\
             0[Keep 7]* 1[Mulligan]\n",
            cards(&play.decision().prompt.about),
        )
    );
    assert_eq!(cards(&play.decision().prompt.about), cards(&hand));
    picked.choose(|c| *c == Candidate::Keep);
    assert_eq!(play.press(option(0)), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);

    // A mulligan is the other button, and the next hand is kept at six.
    let mut play = dealt(seed);
    play.press(option(0));
    assert_eq!(play.press(option(1)), None);
    assert!(play.decision().prompt.is(Purpose::Mulligan));
    assert!(play.prompt().ends_with("0[Keep 6]* 1[Mulligan]\n"));
}
