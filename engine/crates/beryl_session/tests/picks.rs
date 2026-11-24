//! Picks of cards held by where the cards lie, whatever the pick is for: a
//! card chosen among those exiled with a permanent, by clicks alone, and a pick of every
//! purpose from a decision built here, as a card with a purpose new to the list would ask it.
//! The first holds the prompt's state after each click to a text written by hand from what
//! the screen shows and ends where the picks a UI holding the prompt's state itself
//! would send end.

use crate::play;

use beryl_cards::names::{
    COUNTERSPELL, CURRENCY_CONVERTER, FLOODFARM_VERGE, ISLAND, MANA_CRYPT, OUST, PLAINS, REPRIEVE,
    SEACHROME_COAST, SPARAS_HEADQUARTERS, STOCK_UP, STRIP_MINE, TEFERI_HERO_OF_DOMINARIA, TUNDRA,
};
use beryl_engine::card::{Prompt, Purpose, Shape};
use beryl_engine::{Bitboard, Decision, DecisionKind, Seat, SlotId as _, TurnStep, Zone, ZoneKey};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::{ButtonId, Click, Clicked, Held, ShapeBody};
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
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

/// Even two cards in exile are laid out in a tray, as Arena's "Choose One" browser shows
/// cards exiled with Currency Converter: a click picks one, another click takes its place,
/// and Submit confirms.
#[test]
fn two_exiled_cards_are_chosen_from_a_tray_and_confirmed() {
    let mut play = Play::scenario(&converting(&[OUST, PLAINS]));
    to_the_choice(&mut play);
    let (oust, plains) = (exiled(&play, OUST), exiled(&play, PLAINS));
    let converter = play.named(CURRENCY_CONVERTER, ZoneName::Battlefield, Side::You)[0];
    let offered: Vec<ObjectId> = (play.decision().candidates.iter())
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => Some(slot),
            _ => None,
        })
        .collect();
    assert_eq!(offered.len(), 2);
    let tray: String = offered
        .iter()
        .map(|slot| format!("#{slot}"))
        .collect::<Vec<_>>()
        .join(" ");
    let open = format!(
        "pick choose: Choose One\n\
         tray {tray}\n\
         picked 0 of 1\n\
         eligible {}\n\
         confirm(Submit 0)\n",
        cards(&offered),
    );
    assert_eq!(play.prompt(), open);
    // The Converter itself is not one of them, and Submit 0 does nothing while none is picked.
    assert_eq!(play.on(converter).as_deref(), Some(open.as_str()));
    assert_eq!(
        play.press(ButtonId::Confirm).as_deref(),
        Some(open.as_str())
    );
    // Picking Plains selects it; picking Oust replaces it; Submit 1 puts Oust into the graveyard
    // and creates a Rogue token.
    assert_eq!(
        play.on(plains).unwrap(),
        format!(
            "pick choose: Choose One\n\
             tray {tray}\n\
             picked 1 of 1: #{plains}\n\
             eligible #{oust}\n\
             selected #{plains}\n\
             confirm[Submit 1]*\n",
        )
    );
    assert_eq!(
        play.on(oust).unwrap(),
        format!(
            "pick choose: Choose One\n\
             tray {tray}\n\
             picked 1 of 1: #{oust}\n\
             eligible #{plains}\n\
             selected #{oust}\n\
             confirm[Submit 1]*\n",
        )
    );
    let mut picked = Play::scenario(&converting(&[OUST, PLAINS]));
    to_the_choice(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: oust });
    assert_eq!(play.press(ButtonId::Confirm), None, "Submit 1 answers");
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
            let offered: Vec<Subject> = (beryl_engine::Slot::all()
                .filter(|&slot| (among).contains(slot)))
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

/// The human's turn with Teferi, Hero of Dominaria to +1 and seven lands with different
/// names, all tapped: more than the five cards a choice in a graveyard or exile picks where
/// they lie. (The engine offers interchangeable lands with one name once.)
const TEFERI_UNTAPS_SEVEN: Scenario = Scenario::new("untap among seven lands")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[COUNTERSPELL])
    .permanents(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA),
            Permanent::new(PLAINS).tapped(),
            Permanent::new(ISLAND).tapped(),
            Permanent::new(TUNDRA).tapped(),
            Permanent::new(FLOODFARM_VERGE).tapped(),
            Permanent::new(SPARAS_HEADQUARTERS).tapped(),
            Permanent::new(SEACHROME_COAST).tapped(),
            Permanent::new(STRIP_MINE).tapped(),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Teferi's untap among seven lands picks them on the battlefield, where they lie, as Arena
/// does: no tray lays them out over the table, however many lands there are, and a click on a
/// land picks it there.
#[test]
fn an_untap_among_many_lands_picks_them_where_they_lie() {
    let mut play = Play::scenario(&TEFERI_UNTAPS_SEVEN);
    play.choose(|c| matches!(c, Candidate::Activate { ability: 0, .. }));
    while play.decision().kind == Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
    assert!(play.decision().prompt.is(Purpose::Untap));
    let offered = (play.decision().candidates.iter())
        .filter(|c| matches!(c.option, Candidate::Object { .. }))
        .count();
    assert_eq!(offered, 7, "each land is offered");
    let ShapeBody::Pick { tray, picked, .. } = play.decision().prompt.body.clone() else {
        panic!("an untap is a pick the session holds");
    };
    assert!(
        tray.is_empty(),
        "the lands are laid out in a tray: {tray:?}"
    );
    assert_eq!(picked.len(), 2);
    // A land not picked is picked by a click once one picked is unpicked.
    let lands = [PLAINS, ISLAND, TUNDRA, SEACHROME_COAST, STRIP_MINE]
        .map(|name| play.named(name, ZoneName::Battlefield, Side::You)[0]);
    let unpicked = *lands.iter().find(|&&l| !picked.contains(&card(l))).unwrap();
    let Subject::Object { slot: first } = picked[0] else {
        panic!("a land is a card: {picked:?}")
    };
    play.on(first);
    let after = play.on(unpicked).unwrap();
    assert!(!after.contains("tray"), "{after}");
    assert!(after.contains(&format!("#{unpicked}")), "{after}");
}

/// Choosing one of two cards in a graveyard opens in a tray with Submit, while choosing one of
/// two permanents on the battlefield stays on the table and answers on a click.
#[test]
fn a_graveyard_pick_uses_a_tray_while_a_battlefield_pick_stays_on_the_table() {
    const GRAVEYARD_AND_BOARD: Scenario = Scenario::new("graveyard vs battlefield choose")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(MANA_CRYPT)])
        .graveyard(P0, &[OUST, COUNTERSPELL])
        .library(P0, &[PLAINS; 6])
        .library(P1, &[PLAINS; 6]);
    let (table, state) = position(&GRAVEYARD_AND_BOARD);

    let graveyard = state.zone(ZoneKey::Of(Seat::P0, Zone::Graveyard));
    let gy_decision = Decision::objects(
        Seat::P0,
        DecisionKind::ChooseObject,
        graveyard,
        Prompt::new(Purpose::Choose).most(1),
    );
    let gy_view = asked(&table, &state, &gy_decision);
    let gy_held = Held::opening(&gy_view, &state, &table, Some(&gy_decision))
        .expect("graveyard pick is held");
    let ShapeBody::Pick { tray, .. } = &gy_held.view().body else {
        panic!("graveyard pick has pick body");
    };
    assert_eq!(tray.len(), 2, "graveyard cards open in a tray: {tray:?}");
    assert!(
        gy_held
            .view()
            .buttons
            .iter()
            .any(|b| b.id == ButtonId::Confirm && !b.enabled),
        "tray pick waits for Submit: {:?}",
        gy_held.view().buttons
    );

    let permanents = (state.battlefield().iter())
        .fold(Bitboard::EMPTY, |all, &slot| all | Bitboard::single(slot));
    let bf_decision = Decision::objects(
        Seat::P0,
        DecisionKind::ChooseObject,
        permanents,
        Prompt::new(Purpose::Choose).most(1),
    );
    let bf_view = asked(&table, &state, &bf_decision);
    let bf_held = Held::opening(&bf_view, &state, &table, Some(&bf_decision))
        .expect("battlefield pick is held");
    let ShapeBody::Pick { tray, .. } = &bf_held.view().body else {
        panic!("battlefield pick has pick body");
    };
    assert!(
        tray.is_empty(),
        "battlefield permanents stay on the table: {tray:?}"
    );
    assert!(
        bf_held.view().buttons.is_empty(),
        "single battlefield pick answers by click: {:?}",
        bf_held.view().buttons
    );
}
