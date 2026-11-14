//! The split and order shapes' prompts answered by clicks alone: a scry, a look
//! that takes some cards and bottoms the rest, cards put back on the library, and the order
//! of waiting triggers. Each test holds the prompt's state after each click of one worked
//! example to a text written by hand from what the screen shows, and checks that
//! the clicks end where the picks a UI holding the prompt's state itself would send end.

use crate::play;

use beryl_cards::names::{
    AANGS_ICEBERG, COUNTERSPELL, CURRENCY_CONVERTER, ISLAND, JACE_THE_MIND_SCULPTOR, MANA_CRYPT,
    MANA_TITHE, OUST, PLAINS, REPRIEVE, STOCK_UP, TEFERI_HERO_OF_DOMINARIA, THIRST_FOR_DISCOVERY,
    TUNDRA,
};
use beryl_engine::TurnStep;
use beryl_engine::card::Purpose;
use beryl_engine::state::CounterKind;
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::StopsSetting;
use beryl_session::prompt::{ButtonId, Click};
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::script::Subject;
use play::{Play, card, cards};

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

/// A card dropped into pile `pile` before what is at `at` there.
fn drop(slot: ObjectId, pile: u8, at: u16) -> Click {
    Click::Drop {
        on: card(slot),
        pile,
        at,
    }
}

/// The card named `name` in the human's library.
fn in_library(play: &Play, name: &str) -> ObjectId {
    play.named(name, ZoneName::Library, Side::You)[0]
}

/// Passes the human's priority until another decision is open.
fn pass(play: &mut Play) {
    while play.decision().kind == Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
}

/// The human's main phase with Aang's Iceberg to sacrifice for a scry of two, over a
/// Counterspell and the Plains under it.
const SCRY: Scenario = Scenario::new("scry")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(AANGS_ICEBERG),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .library(P0, &[COUNTERSPELL, PLAINS, ISLAND, TUNDRA, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Activates the Iceberg, paying its waterbend with the Islands, to its scry.
fn to_the_scry(play: &mut Play) {
    play.choose(|c| matches!(c, Candidate::Activate { .. }));
    play.choose(|c| *c == Candidate::Done);
    assert!(play.decision().prompt.is(Purpose::Scry));
}

/// A scry opens with its cards on top as they lie. A click sends a card to the other pile, a
/// drop puts it at a place, and the button deals them out: the bottom's cards first, each
/// under the one before.
#[test]
fn a_scry_deals_its_cards_between_the_bottom_and_the_top() {
    let mut play = Play::scenario(&SCRY);
    to_the_scry(&mut play);
    let counterspell = in_library(&play, COUNTERSPELL);
    let plains = in_library(&play, PLAINS);
    let both = cards(&[counterspell, plains]);
    let open = format!(
        "split scry: Scry\n\
         pile bottom \"BOTTOM\":\n\
         pile top \"TOP\": #{counterspell} #{plains}\n\
         eligible {both}\n\
         confirm[Done]*\n"
    );
    assert_eq!(play.prompt(), open);
    // A card that is not one of the two changes nothing.
    let island = play.named(ISLAND, ZoneName::Battlefield, Side::You)[0];
    assert_eq!(play.on(island).as_deref(), Some(open.as_str()));
    // The Counterspell goes to the bottom, and the Plains under it.
    assert_eq!(
        play.on(counterspell).unwrap(),
        format!(
            "split scry: Scry\n\
             pile bottom \"BOTTOM\": #{counterspell}\n\
             pile top \"TOP\": #{plains}\n\
             eligible {both}\n\
             confirm[Done]*\n"
        )
    );
    assert_eq!(
        play.on(plains).unwrap(),
        format!(
            "split scry: Scry\n\
             pile bottom \"BOTTOM\": #{counterspell} #{plains}\n\
             pile top \"TOP\":\n\
             eligible {both}\n\
             confirm[Done]*\n"
        )
    );
    // Dropped above the Counterspell, the Plains is the higher of the two.
    let dropped = format!(
        "split scry: Scry\n\
         pile bottom \"BOTTOM\": #{plains} #{counterspell}\n\
         pile top \"TOP\":\n\
         eligible {both}\n\
         confirm[Done]*\n"
    );
    assert_eq!(play.click(drop(plains, 0, 0)).unwrap(), dropped);
    // Where it already lies, and past the pile's end, a drop does nothing.
    assert_eq!(play.click(drop(plains, 0, 1)).unwrap(), dropped);
    assert_eq!(play.click(drop(plains, 0, 3)).unwrap(), dropped);
    assert_eq!(
        play.decision().prompt.revision,
        3,
        "three clicks changed it"
    );

    // The button bottoms the Plains, then the Counterspell under it, as picking each does:
    // with none left on top the engine asks no more.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&SCRY);
    to_the_scry(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: plains });
    picked.choose(|c| *c == Candidate::Object { slot: counterspell });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.view.players.you.library[..2], [counterspell, plains]);
    // The first pick is the player's answer and the second its continuation.
    assert_eq!(
        choices(&play, before)[..2],
        [
            (SavedKind::ChooseUpTo, false),
            (SavedKind::ChooseUpTo, true)
        ]
    );

    // Both kept on top, the Plains dropped above the Counterspell: the run ends the bottom's
    // picks and takes the Plains for the top, the Counterspell going under it unasked.
    let mut kept = Play::scenario(&SCRY);
    to_the_scry(&mut kept);
    assert_eq!(
        kept.click(drop(plains, 1, 0)).unwrap(),
        format!(
            "split scry: Scry\n\
             pile bottom \"BOTTOM\":\n\
             pile top \"TOP\": #{plains} #{counterspell}\n\
             eligible {both}\n\
             confirm[Done]*\n"
        )
    );
    picked = Play::scenario(&SCRY);
    to_the_scry(&mut picked);
    picked.choose(|c| *c == Candidate::Done);
    picked.choose(|c| *c == Candidate::Object { slot: plains });
    assert_eq!(kept.press(ButtonId::Confirm), None);
    assert_eq!(kept.game.state(), picked.game.state());
    // The human's next decision is in their next turn: the Plains, on top, is the card drawn.
    assert_eq!(kept.named(PLAINS, ZoneName::Hand, Side::You), [plains]);
    assert_eq!(kept.view.players.you.library.last(), Some(&counterspell));
}

/// The human casts Stock Up over an Island, a Plains, a Mana Crypt, a Counterspell and a
/// Reprieve, with a Tundra under them.
const STOCK: Scenario = Scenario::new("arrange")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[STOCK_UP])
    .permanents(
        P0,
        &[
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .library(
        P0,
        &[ISLAND, PLAINS, MANA_CRYPT, COUNTERSPELL, REPRIEVE, TUNDRA],
    )
    .library(P1, &[PLAINS; 6]);

/// Casts Stock Up and passes until it resolves to its look.
fn to_the_look(play: &mut Play) {
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    pass(play);
    assert!(play.decision().prompt.is(Purpose::Arrange));
}

/// A look that takes two opens with every card bound for the bottom, as a pick: a card
/// clicked is selected for the hand and the button counts the cards selected, waiting until
/// the hand's pile holds its two; a third is refused, and a card clicked back goes back to
/// its place among the rest.
#[test]
fn a_look_takes_its_cards_for_the_hand_and_orders_the_rest_on_the_bottom() {
    let mut play = Play::scenario(&STOCK);
    to_the_look(&mut play);
    let [island, plains, crypt, counterspell, reprieve] =
        [ISLAND, PLAINS, MANA_CRYPT, COUNTERSPELL, REPRIEVE].map(|name| in_library(&play, name));
    let all = cards(&[island, plains, crypt, counterspell, reprieve]);
    let rest = |taken: &[ObjectId]| {
        let left: Vec<ObjectId> = [island, plains, crypt, counterspell, reprieve]
            .into_iter()
            .filter(|slot| !taken.contains(slot))
            .collect();
        cards(&left)
    };
    assert_eq!(
        play.prompt(),
        format!(
            "split arrange: Put two of them into your hand.\n\
             pile hand \"Hand\" exactly 2:\n\
             pile bottom \"Bottom\": #{island} #{plains} #{crypt} #{counterspell} #{reprieve}\n\
             eligible {all}\n\
             confirm(Submit 0)\n"
        )
    );
    let one = format!(
        "split arrange: Put two of them into your hand.\n\
         pile hand \"Hand\" exactly 2: #{crypt}\n\
         pile bottom \"Bottom\": #{island} #{plains} #{counterspell} #{reprieve}\n\
         eligible {}\n\
         selected #{crypt}\n\
         confirm(Submit 1)\n",
        rest(&[crypt])
    );
    assert_eq!(play.on(crypt).unwrap(), one);
    // Clicked back, the Crypt goes back between the Plains and the Counterspell.
    assert_eq!(
        play.on(crypt).unwrap(),
        format!(
            "split arrange: Put two of them into your hand.\n\
             pile hand \"Hand\" exactly 2:\n\
             pile bottom \"Bottom\": #{island} #{plains} #{crypt} #{counterspell} #{reprieve}\n\
             eligible {all}\n\
             confirm(Submit 0)\n"
        )
    );
    assert_eq!(play.on(crypt).unwrap(), one);
    let taken = format!(
        "split arrange: Put two of them into your hand.\n\
         pile hand \"Hand\" exactly 2: #{crypt} #{counterspell}\n\
         pile bottom \"Bottom\": #{island} #{plains} #{reprieve}\n\
         eligible {}\n\
         selected {}\n\
         confirm[Submit 2]*\n",
        rest(&[crypt, counterspell]),
        cards(&[crypt, counterspell])
    );
    assert_eq!(play.on(counterspell).unwrap(), taken);
    // The hand takes two: a third is not moved, by a click or by a drop.
    assert_eq!(play.on(reprieve).unwrap(), taken);
    assert_eq!(play.click(drop(reprieve, 0, 0)).unwrap(), taken);
    // The Reprieve to the top of the bottom's pile, and the Island to its end.
    play.click(drop(reprieve, 1, 0));
    assert_eq!(
        play.click(drop(island, 1, 3)).unwrap(),
        format!(
            "split arrange: Put two of them into your hand.\n\
             pile hand \"Hand\" exactly 2: #{crypt} #{counterspell}\n\
             pile bottom \"Bottom\": #{reprieve} #{plains} #{island}\n\
             eligible {}\n\
             selected {}\n\
             confirm[Submit 2]*\n",
            rest(&[crypt, counterspell]),
            cards(&[crypt, counterspell])
        )
    );

    // The button takes the two for the hand, then bottoms the Reprieve and the Plains under
    // it; the Island goes under them unasked.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&STOCK);
    to_the_look(&mut picked);
    for slot in [crypt, counterspell, reprieve, plains] {
        picked.choose(|c| *c == Candidate::Object { slot });
    }
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    let library = &play.view.players.you.library;
    assert_eq!(library[..3], [island, plains, reprieve], "bottom first");
    assert_eq!(library.len(), 4, "the sixth card is on top");
    assert_eq!(
        play.named(MANA_CRYPT, ZoneName::Hand, Side::You),
        [crypt],
        "taken"
    );
    assert_eq!(
        choices(&play, before)[..4],
        [
            (SavedKind::ChooseObject, false),
            (SavedKind::ChooseObject, true),
            (SavedKind::ChooseObject, true),
            (SavedKind::ChooseObject, true),
        ]
    );
}

/// The human's Jace, the Mind Sculptor, to draw two Plains and an Island to a Counterspell
/// and put two cards back.
const PUT_BACK: Scenario = Scenario::new("put back")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[COUNTERSPELL])
    .permanents(P0, &[Permanent::new(JACE_THE_MIND_SCULPTOR)])
    .library(P0, &[PLAINS, PLAINS, ISLAND, TUNDRA, TUNDRA, TUNDRA])
    .library(P1, &[PLAINS; 6]);

/// Activates Jace's 0 and passes until it resolves to the cards put back.
fn to_the_put_back(play: &mut Play) {
    play.choose(|c| matches!(c, Candidate::Activate { ability: 1, .. }));
    pass(play);
    assert!(play.decision().prompt.is(Purpose::PutBack));
}

/// Cards put back open in the hand, their second pile, and the library's top takes exactly
/// two: the first listed is the top card. A card the engine offers another in place of is
/// put back all the same.
#[test]
fn cards_are_put_back_from_the_hand_onto_the_library_in_an_order() {
    let mut play = Play::scenario(&PUT_BACK);
    to_the_put_back(&mut play);
    let hand: Vec<ObjectId> = (play.view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
        .map(|o| o.slot)
        .collect();
    let line = |slots: &[ObjectId]| {
        let slots: Vec<String> = slots.iter().map(|slot| format!(" #{slot}")).collect();
        slots.concat()
    };
    let island = play.named(ISLAND, ZoneName::Hand, Side::You)[0];
    let counterspell = play.named(COUNTERSPELL, ZoneName::Hand, Side::You)[0];
    // The engine offers one of the two Plains for both (architecture §7.2): the other is
    // the one clicked.
    let offered = |slot: ObjectId| {
        (play.decision().candidates.iter()).any(|c| c.option == Candidate::Object { slot })
    };
    let plains = play.named(PLAINS, ZoneName::Hand, Side::You);
    let (twin, plains) = if offered(plains[0]) {
        (plains[0], plains[1])
    } else {
        (plains[1], plains[0])
    };
    assert!(offered(twin) && !offered(plains));
    let all = cards(&hand);
    assert_eq!(
        play.prompt(),
        format!(
            "split put-back: Put two cards on top.\n\
             pile onTop \"Top\" exactly 2:\n\
             pile none \"Hand\":{}\n\
             eligible {all}\n\
             confirm(Done)\n",
            line(&hand)
        )
    );
    let without = |gone: &[ObjectId]| -> Vec<ObjectId> {
        (hand.iter().copied())
            .filter(|slot| !gone.contains(slot))
            .collect()
    };
    assert_eq!(
        play.on(island).unwrap(),
        format!(
            "split put-back: Put two cards on top.\n\
             pile onTop \"Top\" exactly 2: #{island}\n\
             pile none \"Hand\":{}\n\
             eligible {all}\n\
             confirm(Done)\n",
            line(&without(&[island]))
        )
    );
    // The second goes under the first, and the top has its two.
    let rest = line(&without(&[island, plains]));
    let under = format!(
        "split put-back: Put two cards on top.\n\
         pile onTop \"Top\" exactly 2: #{island} #{plains}\n\
         pile none \"Hand\":{rest}\n\
         eligible {all}\n\
         confirm[Done]*\n"
    );
    assert_eq!(play.on(plains).unwrap(), under);
    assert_eq!(play.on(counterspell).unwrap(), under, "a third is refused");
    // Dropped above the Island, the Plains will be the top card.
    assert_eq!(
        play.click(drop(plains, 0, 0)).unwrap(),
        format!(
            "split put-back: Put two cards on top.\n\
             pile onTop \"Top\" exactly 2: #{plains} #{island}\n\
             pile none \"Hand\":{rest}\n\
             eligible {all}\n\
             confirm[Done]*\n"
        )
    );

    // The button puts the Island back, then a Plains on it: each pick goes on the one before.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&PUT_BACK);
    to_the_put_back(&mut picked);
    picked.choose(|c| *c == Candidate::Object { slot: island });
    picked.choose(|c| *c == Candidate::Object { slot: twin });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    let library = &play.view.players.you.library;
    assert_eq!(library[library.len() - 2..], [island, twin], "top last");
    assert_eq!(
        choices(&play, before)[..2],
        [
            (SavedKind::ChooseObject, false),
            (SavedKind::ChooseObject, true)
        ]
    );
}

/// The human has Teferi, Hero of Dominaria's emblem to get, a Currency Converter, and a Thirst
/// for Discovery to draw three cards and discard two: three emblem triggers, each with a
/// target, and a Converter trigger for each card discarded.
const ORDER: Scenario = Scenario::new("order")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIRST_FOR_DISCOVERY, COUNTERSPELL])
    .permanents(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA).counters(CounterKind::Loyalty, 9),
            Permanent::new(CURRENCY_CONVERTER),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
            Permanent::new(ISLAND),
        ],
    )
    .permanents(
        P1,
        &[
            Permanent::new(PLAINS),
            Permanent::new(ISLAND),
            Permanent::new(TUNDRA),
            Permanent::new(MANA_CRYPT),
        ],
    )
    .library(P0, &[REPRIEVE, OUST, MANA_TITHE, PLAINS, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// Under full control: gets the emblem, casts Thirst for Discovery and discards the first two
/// cards offered, to the order of the triggers waiting when it has resolved.
fn to_the_order(play: &mut Play) {
    let stops = StopsSetting {
        full_control: true,
        ..StopsSetting::SMART
    };
    play.view = play.game.set_stops(stops).unwrap();
    let teferi = play.named(TEFERI_HERO_OF_DOMINARIA, ZoneName::Battlefield, Side::You)[0];
    play.choose(|c| matches!(c, Candidate::Activate { slot, ability: 2, .. } if *slot == teferi));
    while play.decision().kind == Kind::Priority {
        let casts = |c: &Candidate| matches!(c, Candidate::Cast { .. });
        let can_cast = play.view.stack.is_empty()
            && (play.decision().candidates.iter()).any(|c| casts(&c.option));
        if can_cast {
            play.choose(casts);
        } else {
            play.choose(|c| *c == Candidate::Pass);
        }
    }
    assert!(play.decision().prompt.is(Purpose::Discard));
    play.choose(|c| matches!(c, Candidate::Object { .. }));
    play.choose(|c| matches!(c, Candidate::Object { .. }));
    assert!(play.decision().prompt.is(Purpose::TriggerOrder));
}

/// A waiting trigger of an order, by the index of its candidate.
const fn trigger(index: u32) -> Subject {
    Subject::Ability { id: index }
}

/// An order of triggers lists them as they will resolve, in the order the session itself
/// would put them in, and a drop moves one. The button puts them on the stack from the last
/// to resolve up, and the run goes on after the targets a trigger asks for as it goes on.
#[test]
fn triggers_are_ordered_by_drops_and_the_run_waits_through_their_targets() {
    let mut play = Play::scenario(&ORDER);
    to_the_order(&mut play);
    // The engine offers one of the three emblem triggers, which are alike, and the
    // Converter's two. The emblem's has a target and the Converter's ask as they resolve, so
    // the session would resolve the emblem's first.
    let open = "order trigger-order: Order Triggers\n\
                order a0 a1 a2\n\
                confirm[Done]*\n";
    assert_eq!(play.prompt(), open);
    let moved = |on: u32, at: u16| Click::Drop {
        on: trigger(on),
        pile: 0,
        at,
    };
    // Where it already lies, a drop does nothing; nor does a click on a card.
    assert_eq!(play.click(moved(0, 0)).as_deref(), Some(open));
    assert_eq!(play.click(moved(0, 1)).as_deref(), Some(open));
    assert_eq!(play.click(moved(1, 4)).as_deref(), Some(open));
    let teferi = play.named(TEFERI_HERO_OF_DOMINARIA, ZoneName::Battlefield, Side::You)[0];
    assert_eq!(play.on(teferi).as_deref(), Some(open));
    // The emblem's trigger to the end: it resolves last, so it goes on the stack first.
    assert_eq!(
        play.click(moved(0, 3)).unwrap(),
        "order trigger-order: Order Triggers\n\
         order a1 a2 a0\n\
         confirm[Done]*\n"
    );
    assert_eq!(
        play.click(moved(2, 0)).unwrap(),
        "order trigger-order: Order Triggers\n\
         order a2 a1 a0\n\
         confirm[Done]*\n"
    );

    // The button puts the emblem's trigger on the stack, as picking it does, and its target
    // is a prompt of its own.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&ORDER);
    to_the_order(&mut picked);
    picked.choose(|c| matches!(c, Candidate::Trigger { source: None, .. }));
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert!(play.decision().prompt.is(Purpose::Target));
    assert_eq!(play.game.state(), picked.game.state());
    // Once the target is chosen the run goes on: the session puts a Converter trigger on the
    // stack unasked, as the pick of one does. Two emblem triggers and the other Converter
    // trigger still wait, so their order is asked next.
    let target = play.named(MANA_CRYPT, ZoneName::Battlefield, Side::Opponent)[0];
    picked.choose(|c| *c == Candidate::Object { slot: target });
    picked.choose(|c| {
        matches!(
            c,
            Candidate::Trigger {
                source: Some(_),
                ..
            }
        )
    });
    assert_eq!(play.on(target), None);
    assert_eq!(
        play.prompt(),
        "order trigger-order: Order Triggers\n\
         order a0 a1\n\
         confirm[Done]*\n"
    );
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        choices(&play, before),
        [
            (SavedKind::TriggerOrder, false),
            (SavedKind::Target, false),
            (SavedKind::TriggerOrder, true),
        ]
    );
}

/// An undo while a run waits takes the run back with the game: the order is asked afresh,
/// and the pick after the target is the player's again.
#[test]
fn an_undo_drops_a_run_waiting_through_a_target() {
    let mut play = Play::scenario(&ORDER);
    to_the_order(&mut play);
    play.click(Click::Drop {
        on: trigger(0),
        pile: 0,
        at: 3,
    });
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert!(play.decision().prompt.is(Purpose::Target));
    play.view = play.game.undo().unwrap();
    assert_eq!(
        play.prompt(),
        "order trigger-order: Order Triggers\n\
         order a0 a1 a2\n\
         confirm[Done]*\n"
    );
    // Answered pick by pick this time, the order after the target is asked.
    play.choose(|c| matches!(c, Candidate::Trigger { source: None, .. }));
    let target = play.named(MANA_CRYPT, ZoneName::Battlefield, Side::Opponent)[0];
    play.choose(|c| *c == Candidate::Object { slot: target });
    assert_eq!(
        play.prompt(),
        "order trigger-order: Order Triggers\n\
         order a0 a1 a2\n\
         confirm[Done]*\n"
    );
}
