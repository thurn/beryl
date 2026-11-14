//! Picks of cards held by where the cards lie, whatever the pick is for: a
//! card chosen among those exiled with a permanent, by clicks alone, and a pick of every
//! purpose from a decision built here, as a card with a purpose new to the list would ask it.
//! The first holds the prompt's state after each click to a text written by hand from what
//! the screen shows and ends where the picks a UI holding the prompt's state itself
//! would send end.

use crate::play;

use beryl_cards::names::{
    COUNTERSPELL, CURRENCY_CONVERTER, ISLAND, MANA_CRYPT, OUST, PLAINS, REPRIEVE, STOCK_UP, TUNDRA,
};
use beryl_engine::card::{Prompt, Purpose, Shape};
use beryl_engine::{Bitboard, Decision, DecisionKind, Seat, SlotId as _, TurnStep, Zone, ZoneKey};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::{ButtonId, Click, Clicked, Held, ShapeBody};
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Side, ZoneName};
use beryl_session::script::{Mark, Subject};
use play::{Play, asked, card, cards, position};

/// The human's main phase with a Currency Converter and the cards `exiled` with it.
const fn converting(exiled: &'static [&'static str]) -> Scenario {
    const CONVERTER: Permanent = Permanent::new(CURRENCY_CONVERTER);
    // A scenario's permanents are a static slice, so each list of exiled cards has its own.
    const TWO: &[Permanent] = &[CONVERTER.exiled_with(&[OUST, PLAINS])];
    const SIX: &[Permanent] =
        &[CONVERTER.exiled_with(&[OUST, PLAINS, REPRIEVE, STOCK_UP, TUNDRA, COUNTERSPELL])];
    Scenario::new("choose")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, if exiled.len() == 2 { TWO } else { SIX })
        .library(P0, &[ISLAND; 6])
        .library(P1, &[ISLAND; 6])
}

/// Plays to the choice of a card exiled with the Converter: its last ability, activated and
/// resolved.
fn to_the_choice(play: &mut Play) {
    play.choose(|c| matches!(c, Candidate::Activate { .. }));
    while !play.decision().prompt.is(Purpose::Choose) {
        play.choose(|c| *c == Candidate::Pass);
    }
}

fn exiled(play: &Play, name: &str) -> ObjectId {
    play.named(name, ZoneName::Exile, Side::You)[0]
}

/// A few cards in a zone every player sees are picked where they lie, and one click on one
/// answers: nothing is left to confirm.
#[test]
fn one_of_a_few_exiled_cards_is_chosen_by_a_click_on_it() {
    let mut play = Play::scenario(&converting(&[OUST, PLAINS]));
    to_the_choice(&mut play);
    let (oust, plains) = (exiled(&play, OUST), exiled(&play, PLAINS));
    let converter = play.named(CURRENCY_CONVERTER, ZoneName::Battlefield, Side::You)[0];
    let open = format!(
        "pick choose: Choose One\n\
         picked 0 of 1\n\
         eligible {}\n",
        cards(&[oust, plains]),
    );
    assert_eq!(play.prompt(), open);
    // The Converter itself is not one of them.
    assert_eq!(play.on(converter).as_deref(), Some(open.as_str()));
    // Oust is not a land: it goes to the graveyard and a Rogue comes.
    let mut picked = Play::scenario(&converting(&[OUST, PLAINS]));
    to_the_choice(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: oust });
    assert_eq!(play.on(oust), None, "the click answers");
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.named(OUST, ZoneName::Graveyard, Side::You).len(), 1);
    assert_eq!(play.named(PLAINS, ZoneName::Exile, Side::You).len(), 1);
}

/// More cards than the table shows at a glance are laid out in a tray: a click picks one,
/// another click takes its place, and the button answers.
#[test]
fn one_of_many_exiled_cards_is_chosen_from_a_tray_and_confirmed() {
    let scenario = converting(&[OUST, PLAINS, REPRIEVE, STOCK_UP, TUNDRA, COUNTERSPELL]);
    let mut play = Play::scenario(&scenario);
    to_the_choice(&mut play);
    let (oust, plains) = (exiled(&play, OUST), exiled(&play, PLAINS));
    // The tray lists the six as the decision offers them.
    let offered: Vec<ObjectId> = (play.decision().candidates.iter())
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => Some(slot),
            _ => None,
        })
        .collect();
    assert_eq!(offered.len(), 6);
    let tray: Vec<String> = offered.iter().map(|slot| format!("#{slot}")).collect();
    let tray = tray.join(" ");
    assert_eq!(
        play.prompt(),
        format!(
            "pick choose: Choose One\n\
             tray {tray}\n\
             picked 0 of 1\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&offered),
        )
    );
    // Nothing is picked: the button does nothing.
    let open = play.prompt();
    assert_eq!(
        play.press(ButtonId::Confirm).as_deref(),
        Some(open.as_str())
    );
    let others = |picked: ObjectId| {
        let rest: Vec<ObjectId> = offered.iter().copied().filter(|&s| s != picked).collect();
        cards(&rest)
    };
    assert_eq!(
        play.on(oust).unwrap(),
        format!(
            "pick choose: Choose One\n\
             tray {tray}\n\
             picked 1 of 1: #{oust}\n\
             eligible {}\n\
             selected #{oust}\n\
             confirm[Submit 1]*\n",
            others(oust),
        )
    );
    // The Plains takes Oust's place.
    assert_eq!(
        play.on(plains).unwrap(),
        format!(
            "pick choose: Choose One\n\
             tray {tray}\n\
             picked 1 of 1: #{plains}\n\
             eligible {}\n\
             selected #{plains}\n\
             confirm[Submit 1]*\n",
            others(plains),
        )
    );
    // A land: it goes to the graveyard and a Treasure comes.
    let mut picked = Play::scenario(&scenario);
    to_the_choice(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: plains });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(play.named(PLAINS, ZoneName::Graveyard, Side::You).len(), 1);
}

/// Two permanents and two cards in hand to pick among.
const CARDS_TO_PICK: Scenario = Scenario::new("any pick")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(MANA_CRYPT)])
    .hand(P0, &[OUST, COUNTERSPELL])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// The session holds a pick of cards for any purpose, from the decision alone: where the cards
/// lie says how they are picked, so a purpose added to the list needs no entry in the session
/// to be answered by clicks. Each pick here takes two cards, of two permanents and
/// then of two cards in hand, and is confirmed by its button.
#[test]
fn a_pick_of_cards_for_any_purpose_is_answered_by_clicks() {
    let (table, state) = position(&CARDS_TO_PICK);
    let permanents = (state.battlefield().iter())
        .fold(Bitboard::EMPTY, |all, &slot| all | Bitboard::single(slot));
    let hand = state.zone(ZoneKey::Of(Seat::P0, Zone::Hand));
    let confirm = Click::Button {
        button: ButtonId::Confirm,
    };
    for &purpose in Purpose::ALL {
        // A priority, targets and a division of combat damage are answered in forms of their
        // own, from candidates of their own.
        let own_form = matches!(
            purpose,
            Purpose::Priority | Purpose::Target | Purpose::CombatDamage
        );
        if purpose.shape() != Shape::Pick || own_form {
            continue;
        }
        for among in [permanents, hand] {
            let d = Decision::objects(
                Seat::P0,
                DecisionKind::ChooseObject,
                among,
                Prompt::new(purpose).most(2),
            );
            let view = asked(&table, &state, &d);
            let mut held = Held::opening(&view, &state, &table, Some(&d))
                .unwrap_or_else(|| panic!("the session does not hold a pick for {purpose:?}"));
            let offered: Vec<Subject> = (state.in_candidate_order(among))
                .map(|slot| card(ObjectId(slot.num())))
                .collect();
            assert_eq!(offered.len(), 2);
            // Each card offered is marked, as one to pick or as one a prompt opens with.
            let marked = |held: &Held, on: Subject| {
                let prompt = held.view();
                (prompt.marks.iter()).find_map(|m| (m.on == on).then_some(m.mark))
            };
            for &on in &offered {
                if marked(&held, on) == Some(Mark::Eligible) {
                    assert!(
                        matches!(
                            held.click(&state, &table, Some(&d), Click::On { on }),
                            Clicked::Changed
                        ),
                        "{purpose:?}: a click on a card to pick does not pick it"
                    );
                }
                assert_eq!(marked(&held, on), Some(Mark::Selected), "{purpose:?}");
            }
            let ShapeBody::Pick { picked, .. } = &held.view().body else {
                panic!("{purpose:?} is not held as a pick");
            };
            assert_eq!(picked.len(), 2, "{purpose:?}");
            let button = (held.view().buttons.iter()).find(|b| b.id == ButtonId::Confirm);
            assert!(
                button.is_some_and(|b| b.enabled && !b.label.is_empty()),
                "{purpose:?}: {:?}",
                held.view().buttons
            );
            assert!(
                matches!(
                    held.click(&state, &table, Some(&d), confirm),
                    Clicked::Answers(_)
                ),
                "{purpose:?}: the button does not answer"
            );
        }
    }
}
