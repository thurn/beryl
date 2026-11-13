//! Picks of cards from a hand, a library or a graveyard answered by clicks alone:
//! a search, a look that may take a card, discards, cards exiled for a cost, the cards put on
//! the bottom after a mulligan and a manifest dread. Each test holds the prompt's state after
//! each click of one worked example to a text written by hand from what the screen shows
//!, and checks that the clicks end where the picks a UI holding the prompt's state
//! itself would send end.

#[path = "common/play.rs"]
mod play;

use beryl_cards::names::{
    ABHORRENT_OCULUS, COUNTERSPELL, CURRENCY_CONVERTER, ELITE_SPELLBINDER, ISLAND, MANA_CRYPT,
    MANA_DRAIN, MANA_TITHE, OUST, PLAINS, REPRIEVE, SNAPCASTER_MAGE, STOCK_UP, SUBTLETY,
    THIRST_FOR_DISCOVERY, THUNDERTRAP_TRAINER, TRINKET_MAGE, TUNDRA,
};
use beryl_engine::card::Purpose;
use beryl_engine::{Seat, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::prompt::ButtonId;
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, Kind, Side, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::save::Kind as SavedKind;
use beryl_session::{StartGame, StopsSetting};
use play::{Play, cards};

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

/// Passes the human's priority until another decision is open.
fn pass(play: &mut Play) {
    while play.decision().kind == Kind::Priority {
        play.choose(|c| *c == Candidate::Pass);
    }
}

/// Casts the one card the human can and passes until a prompt for `purpose` is open.
fn cast_to(play: &mut Play, purpose: Purpose) {
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    pass(play);
    assert!(play.decision().prompt.is(purpose));
}

/// Picks the card in `slot`, as a UI that holds the prompt's state itself does.
fn pick(play: &mut Play, slot: ObjectId) {
    play.choose(|c| *c == Candidate::Object { slot });
}

/// The human's cards named `name` in `zone`.
fn mine(play: &Play, name: &str, zone: ZoneName) -> Vec<ObjectId> {
    play.named(name, zone, Side::You)
}

const ISLANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];

/// The human casts Trinket Mage with two Mana Crypts and a Currency Converter to find.
const SEARCH: Scenario = Scenario::new("search")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[TRINKET_MAGE])
    .permanents(P0, ISLANDS)
    .library(
        P0,
        &[MANA_CRYPT, PLAINS, CURRENCY_CONVERTER, MANA_CRYPT, PLAINS],
    )
    .library(P1, &[PLAINS; 6]);

/// As [`SEARCH`], with one card to find.
const SEARCH_FOR_ONE: Scenario = Scenario::new("search for the one")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[TRINKET_MAGE])
    .permanents(P0, ISLANDS)
    .library(P0, &[PLAINS, MANA_CRYPT, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// A search lays out what it can find, by name, each copy to pick though the engine offers
/// one for both. Its button finds the card picked; the other finds nothing. No yes or no
/// comes before it.
#[test]
fn a_search_lays_out_what_it_finds_and_may_find_nothing() {
    let mut play = Play::scenario(&SEARCH);
    cast_to(&mut play, Purpose::Search);
    let crypts = mine(&play, MANA_CRYPT, ZoneName::Library);
    let converter = mine(&play, CURRENCY_CONVERTER, ZoneName::Library)[0];
    let (first, second) = (crypts[0], crypts[1]);
    let open = format!(
        "pick search: Search for a card.\n\
         tray #{converter} #{first} #{second}\n\
         picked 0 of 0..1\n\
         eligible {}\n\
         confirm(Submit 0) decline[Decline]*\n",
        cards(&[converter, first, second]),
    );
    assert_eq!(play.prompt(), open);
    // A card the search cannot find is not one to pick.
    let plains = mine(&play, PLAINS, ZoneName::Library)[0];
    assert_eq!(play.on(plains).as_deref(), Some(open.as_str()));
    assert_eq!(
        play.on(second).unwrap(),
        format!(
            "pick search: Search for a card.\n\
             tray #{converter} #{first} #{second}\n\
             picked 1 of 0..1: #{second}\n\
             eligible {}\n\
             selected #{second}\n\
             confirm[Submit 1]* decline[Decline]\n",
            cards(&[converter, first]),
        )
    );
    // Another pick takes its place.
    assert_eq!(
        play.on(converter).unwrap(),
        format!(
            "pick search: Search for a card.\n\
             tray #{converter} #{first} #{second}\n\
             picked 1 of 0..1: #{converter}\n\
             eligible {}\n\
             selected #{converter}\n\
             confirm[Submit 1]* decline[Decline]\n",
            cards(&[first, second]),
        )
    );
    // The button finds the Converter, as picking it does: the engine asks for no end once
    // the one card is found.
    let mut picked = Play::scenario(&SEARCH);
    cast_to(&mut picked, Purpose::Search);
    pick(&mut picked, converter);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log, picked.game.replay().log);
    assert_eq!(mine(&play, CURRENCY_CONVERTER, ZoneName::Hand).len(), 1);

    // Finding nothing is the other button; a pick made is dropped.
    let mut nothing = Play::scenario(&SEARCH);
    cast_to(&mut nothing, Purpose::Search);
    nothing.on(second);
    picked = Play::scenario(&SEARCH);
    cast_to(&mut picked, Purpose::Search);
    picked.choose(|c| *c == Candidate::Done);
    assert_eq!(nothing.press(ButtonId::Decline), None);
    assert_eq!(nothing.game.state(), picked.game.state());

    // The one card a search can find opens picked.
    let mut one = Play::scenario(&SEARCH_FOR_ONE);
    cast_to(&mut one, Purpose::Search);
    let crypt = mine(&one, MANA_CRYPT, ZoneName::Library)[0];
    assert_eq!(
        one.prompt(),
        format!(
            "pick search: Search for a card.\n\
             tray #{crypt}\n\
             picked 1 of 0..1: #{crypt}\n\
             selected #{crypt}\n\
             confirm[Submit 1]* decline[Decline]\n"
        )
    );
}

/// The human casts Thundertrap Trainer over a Plains, a Counterspell, an Island and a
/// Reprieve.
const LOOK: Scenario = Scenario::new("look")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THUNDERTRAP_TRAINER])
    .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
    .library(
        P0,
        &[PLAINS, COUNTERSPELL, ISLAND, REPRIEVE, PLAINS, PLAINS],
    )
    .library(P1, &[PLAINS; 6]);

/// A look at the top of a library that may take a card is one prompt: the cards looked at,
/// those it can take to pick, a button that takes the one picked and one that takes nothing.
#[test]
fn a_look_takes_a_card_or_nothing_in_one_prompt() {
    let mut play = Play::scenario(&LOOK);
    cast_to(&mut play, Purpose::Look);
    let library = |play: &Play, name: &str| mine(play, name, ZoneName::Library)[0];
    let [plains, counterspell, island, reprieve] =
        [PLAINS, COUNTERSPELL, ISLAND, REPRIEVE].map(|name| library(&play, name));
    let tray = format!("tray #{plains} #{counterspell} #{island} #{reprieve}");
    let open = format!(
        "pick look: Put one of them into your hand?\n\
         {tray}\n\
         picked 0 of 0..1\n\
         eligible {}\n\
         confirm(Submit 0) decline[Decline]*\n",
        cards(&[counterspell, reprieve]),
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(play.decision().kind, Kind::YesNo, "the engine asks whether");
    // A land is looked at, not taken.
    assert_eq!(play.on(plains).as_deref(), Some(open.as_str()));
    assert_eq!(
        play.on(counterspell).unwrap(),
        format!(
            "pick look: Put one of them into your hand?\n\
             {tray}\n\
             picked 1 of 0..1: #{counterspell}\n\
             eligible #{reprieve}\n\
             selected #{counterspell}\n\
             confirm[Submit 1]* decline[Decline]\n"
        )
    );
    // The button answers yes, then the card.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&LOOK);
    cast_to(&mut picked, Purpose::Look);
    picked.choose(|c| *c == Candidate::Yes);
    pick(&mut picked, counterspell);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(mine(&play, COUNTERSPELL, ZoneName::Hand), [counterspell]);
    assert_eq!(
        choices(&play, before)[..2],
        [(SavedKind::YesNo, false), (SavedKind::ChooseObject, true)]
    );

    // Taking nothing answers no.
    let mut nothing = Play::scenario(&LOOK);
    cast_to(&mut nothing, Purpose::Look);
    picked = Play::scenario(&LOOK);
    cast_to(&mut picked, Purpose::Look);
    picked.choose(|c| *c == Candidate::No);
    assert_eq!(nothing.press(ButtonId::Decline), None);
    assert_eq!(nothing.game.state(), picked.game.state());

    // A game taken up after the yes opens the prompt at its pick: a card must be taken.
    let mut later = Play::scenario(&LOOK);
    cast_to(&mut later, Purpose::Look);
    later.choose(|c| *c == Candidate::Yes);
    assert_eq!(
        later.prompt(),
        format!(
            "pick look: Put one of them into your hand.\n\
             {tray}\n\
             picked 0 of 1\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&[counterspell, reprieve]),
        )
    );
    later.on(reprieve);
    picked = Play::scenario(&LOOK);
    cast_to(&mut picked, Purpose::Look);
    picked.choose(|c| *c == Candidate::Yes);
    pick(&mut picked, reprieve);
    assert_eq!(later.press(ButtonId::Confirm), None);
    assert_eq!(later.game.state(), picked.game.state());
}

/// The human casts Elite Spellbinder at an opponent holding a Plains and a Counterspell.
const PEEK: Scenario = Scenario::new("look at a hand")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[ELITE_SPELLBINDER])
    .hand(P1, &[PLAINS, COUNTERSPELL])
    .permanents(
        P0,
        &[
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
            Permanent::new(PLAINS),
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Casts Elite Spellbinder, whose one target the engine takes, and passes to its look at the
/// opponent's hand.
fn to_the_peek(play: &mut Play) {
    cast_to(play, Purpose::Look);
}

/// A look at an opponent's hand lays the hand out, and the one card it can take opens
/// picked.
#[test]
fn a_look_at_a_hand_opens_with_its_one_card_picked() {
    let mut play = Play::scenario(&PEEK);
    to_the_peek(&mut play);
    let theirs = |name: &str| play.named(name, ZoneName::Hand, Side::Opponent)[0];
    let (plains, counterspell) = (theirs(PLAINS), theirs(COUNTERSPELL));
    let hand: Vec<String> = (play.view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::Opponent)
        .map(|o| format!(" #{}", o.slot))
        .collect();
    assert_eq!(hand.len(), 2);
    assert!(hand.contains(&format!(" #{plains}")));
    assert_eq!(
        play.prompt(),
        format!(
            "pick look: Exile one of them?\n\
             tray{}\n\
             picked 1 of 0..1: #{counterspell}\n\
             selected #{counterspell}\n\
             confirm[Submit 1]* decline[Decline]\n",
            hand.concat()
        )
    );
    // The button exiles it, as a yes does: with one card to take, the engine asks no more.
    let mut picked = Play::scenario(&PEEK);
    to_the_peek(&mut picked);
    picked.choose(|c| *c == Candidate::Yes);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        play.named(COUNTERSPELL, ZoneName::Exile, Side::Opponent),
        [counterspell]
    );
}

/// The human casts Thirst for Discovery, drawing an Island, a Plains and a Reprieve to a
/// Counterspell.
const THIRST: Scenario = Scenario::new("discard")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[THIRST_FOR_DISCOVERY, COUNTERSPELL])
    .permanents(P0, ISLANDS)
    .library(P0, &[ISLAND, PLAINS, REPRIEVE, PLAINS, PLAINS, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// A discard of two cards unless a basic land is discarded is one prompt over the whole
/// hand: a basic land alone answers it, or two cards.
#[test]
fn a_discard_takes_a_basic_land_alone_or_two_cards() {
    let mut play = Play::scenario(&THIRST);
    cast_to(&mut play, Purpose::Discard);
    let hand = |name: &str| play.named(name, ZoneName::Hand, Side::You)[0];
    let [island, plains, reprieve, counterspell] =
        [ISLAND, PLAINS, REPRIEVE, COUNTERSPELL].map(hand);
    let open = format!(
        "pick discard: Discard a basic land card or two cards.\n\
         picked 0 of 2\n\
         eligible {}\n\
         confirm(Submit 0)\n",
        cards(&[island, plains, reprieve, counterspell]),
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(
        play.decision().kind,
        Kind::YesNo,
        "the engine asks which way"
    );
    // A card that is no basic land does not answer alone.
    assert_eq!(
        play.on(counterspell).unwrap(),
        format!(
            "pick discard: Discard a basic land card or two cards.\n\
             picked 1 of 2: #{counterspell}\n\
             eligible {}\n\
             selected #{counterspell}\n\
             confirm(Submit 1)\n",
            cards(&[island, plains, reprieve]),
        )
    );
    // Two cards answer, and no third can be picked.
    let two = format!(
        "pick discard: Discard a basic land card or two cards.\n\
         picked 2 of 2: #{counterspell} #{reprieve}\n\
         selected {}\n\
         confirm[Submit 2]*\n",
        cards(&[counterspell, reprieve]),
    );
    assert_eq!(play.on(reprieve).unwrap(), two);
    assert_eq!(play.on(island).unwrap(), two);
    // The button answers no, then discards the two.
    let before = play.game.replay().log.len();
    let mut picked = Play::scenario(&THIRST);
    cast_to(&mut picked, Purpose::Discard);
    picked.choose(|c| *c == Candidate::No);
    pick(&mut picked, counterspell);
    pick(&mut picked, reprieve);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        choices(&play, before)[..3],
        [
            (SavedKind::YesNo, false),
            (SavedKind::ChooseObject, true),
            (SavedKind::ChooseObject, true),
        ]
    );
}

/// A basic land alone answers a discard that takes one in place of two cards; and a game
/// taken up after the answer was no opens a plain discard of two.
#[test]
fn a_basic_land_alone_answers_a_discard_and_a_plain_discard_counts_its_cards() {
    let mut land = Play::scenario(&THIRST);
    cast_to(&mut land, Purpose::Discard);
    let hand = |name: &str| land.named(name, ZoneName::Hand, Side::You)[0];
    let [island, plains, reprieve, counterspell] =
        [ISLAND, PLAINS, REPRIEVE, COUNTERSPELL].map(hand);
    // Yes, then that land of the two.
    assert_eq!(
        land.on(island).unwrap(),
        format!(
            "pick discard: Discard a basic land card or two cards.\n\
             picked 1 of 2: #{island}\n\
             eligible {}\n\
             selected #{island}\n\
             confirm[Submit 1]*\n",
            cards(&[plains, reprieve, counterspell]),
        )
    );
    let mut picked = Play::scenario(&THIRST);
    cast_to(&mut picked, Purpose::Discard);
    picked.choose(|c| *c == Candidate::Yes);
    pick(&mut picked, island);
    assert_eq!(land.press(ButtonId::Confirm), None);
    assert_eq!(land.game.state(), picked.game.state());
    assert_eq!(mine(&land, ISLAND, ZoneName::Graveyard), [island]);

    // A game taken up after the no opens a discard of two, counted on its button.
    let mut later = Play::scenario(&THIRST);
    cast_to(&mut later, Purpose::Discard);
    later.choose(|c| *c == Candidate::No);
    assert_eq!(
        later.prompt(),
        format!(
            "pick discard: Discard two cards.\n\
             picked 0 of 2\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&[island, plains, reprieve, counterspell]),
        )
    );
    later.on(plains);
    assert_eq!(
        later.on(island).unwrap(),
        format!(
            "pick discard: Discard two cards.\n\
             picked 2 of 2: #{plains} #{island}\n\
             selected {}\n\
             confirm[Submit 2]*\n",
            cards(&[plains, island]),
        )
    );
    picked = Play::scenario(&THIRST);
    cast_to(&mut picked, Purpose::Discard);
    picked.choose(|c| *c == Candidate::No);
    pick(&mut picked, plains);
    pick(&mut picked, island);
    assert_eq!(later.press(ButtonId::Confirm), None);
    assert_eq!(later.game.state(), picked.game.state());
}

/// The human's end step with nine cards in hand, two of them Islands.
const CLEANUP: Scenario = Scenario::new("cleanup")
    .at(3, P0, TurnStep::End)
    .hand(
        P0,
        &[
            ISLAND,
            ISLAND,
            PLAINS,
            COUNTERSPELL,
            REPRIEVE,
            OUST,
            MANA_TITHE,
            TUNDRA,
            STOCK_UP,
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// The cleanup step's discard takes its cards from the hand where they lie and counts them
/// on its button. Either of two cards alike is picked, though the engine offers one.
#[test]
fn a_cleanup_discards_down_to_the_hand_size_by_its_button() {
    let mut play = Play::scenario(&CLEANUP);
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::Cleanup));
    let hand: Vec<ObjectId> = (play.view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
        .map(|o| o.slot)
        .collect();
    assert_eq!(hand.len(), 9);
    let islands = mine(&play, ISLAND, ZoneName::Hand);
    let offered = |play: &Play, slot: ObjectId| {
        (play.decision().candidates.iter()).any(|c| c.option == Candidate::Object { slot })
    };
    let (twin, island) = if offered(&play, islands[0]) {
        (islands[0], islands[1])
    } else {
        (islands[1], islands[0])
    };
    assert!(!offered(&play, island), "one Island is offered for both");
    assert_eq!(
        play.prompt(),
        format!(
            "pick cleanup: Discard 2 cards.\n\
             picked 0 of 2\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&hand),
        )
    );
    let rest = |gone: &[ObjectId]| -> Vec<ObjectId> {
        (hand.iter().copied())
            .filter(|slot| !gone.contains(slot))
            .collect()
    };
    assert_eq!(
        play.on(island).unwrap(),
        format!(
            "pick cleanup: Discard 2 cards.\n\
             picked 1 of 2: #{island}\n\
             eligible {}\n\
             selected #{island}\n\
             confirm(Submit 1)\n",
            cards(&rest(&[island])),
        )
    );
    assert_eq!(
        play.on(twin).unwrap(),
        format!(
            "pick cleanup: Discard 2 cards.\n\
             picked 2 of 2: #{island} #{twin}\n\
             selected {}\n\
             confirm[Submit 2]*\n",
            cards(&[island, twin]),
        )
    );
    // The button discards both Islands, each as the engine offers it.
    let mut picked = Play::scenario(&CLEANUP);
    pass(&mut picked);
    for _ in 0..2 {
        let offered = (picked.decision().candidates.iter())
            .find_map(|c| match c.option {
                Candidate::Object { slot } if islands.contains(&slot) => Some(slot),
                _ => None,
            })
            .expect("an Island is offered");
        pick(&mut picked, offered);
    }
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(mine(&play, ISLAND, ZoneName::Graveyard).len(), 2);
}

/// The human evokes Subtlety with a Counterspell and a Mana Drain to exile for it.
const EVOKE: Scenario = Scenario::new("evoke")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[SUBTLETY, COUNTERSPELL, MANA_DRAIN, PLAINS])
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// A card exiled from the hand for a cost is picked where it lies and confirmed by a button,
/// not taken by the click.
#[test]
fn a_card_is_exiled_from_the_hand_for_a_cost_by_a_button() {
    let mut play = Play::scenario(&EVOKE);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    assert!(play.decision().prompt.is(Purpose::ExileCost));
    let counterspell = mine(&play, COUNTERSPELL, ZoneName::Hand)[0];
    let drain = mine(&play, MANA_DRAIN, ZoneName::Hand)[0];
    let plains = mine(&play, PLAINS, ZoneName::Hand)[0];
    let open = format!(
        "pick exile-cost: Exile a card.\n\
         picked 0 of 1\n\
         eligible {}\n\
         confirm(Submit 0)\n",
        cards(&[counterspell, drain]),
    );
    assert_eq!(play.prompt(), open);
    assert_eq!(play.on(plains).as_deref(), Some(open.as_str()), "not blue");
    assert_eq!(
        play.on(drain).unwrap(),
        format!(
            "pick exile-cost: Exile a card.\n\
             picked 1 of 1: #{drain}\n\
             eligible #{counterspell}\n\
             selected #{drain}\n\
             confirm[Submit 1]*\n"
        )
    );
    let mut picked = Play::scenario(&EVOKE);
    picked.choose(|c| matches!(c, Candidate::Cast { .. }));
    pick(&mut picked, drain);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(mine(&play, MANA_DRAIN, ZoneName::Exile), [drain]);
}

/// The human casts Abhorrent Oculus with eight cards in the graveyard, from the top down:
/// two Islands, a Plains, a Counterspell, a Plains, a Reprieve, a Mana Tithe and an Oust.
const DELVE: Scenario = Scenario::new("exile from a graveyard")
    .at(3, P0, TurnStep::PrecombatMain)
    .hand(P0, &[ABHORRENT_OCULUS])
    .permanents(P0, ISLANDS)
    .graveyard(
        P0,
        &[
            ISLAND,
            ISLAND,
            PLAINS,
            COUNTERSPELL,
            PLAINS,
            REPRIEVE,
            MANA_TITHE,
            OUST,
        ],
    )
    .library(P0, &[PLAINS; 6])
    .library(P1, &[PLAINS; 6]);

/// Cards exiled from a graveyard for a cost are laid out, any of them to pick. The engine
/// takes them from the bottom up, the lowest of cards alike, so the picks the button sends
/// name the lower Plains and the lower Island for the upper ones picked.
#[test]
fn cards_are_exiled_from_a_graveyard_for_a_cost_from_the_bottom_up() {
    let mut play = Play::scenario(&DELVE);
    play.choose(|c| matches!(c, Candidate::Cast { .. }));
    assert!(play.decision().prompt.is(Purpose::ExileCost));
    // Bottom first.
    let graveyard = play.view.players.you.graveyard.clone();
    let [
        oust,
        tithe,
        reprieve,
        low_plains,
        counterspell,
        plains,
        low_island,
        island,
    ] = graveyard[..]
    else {
        panic!("eight cards in the graveyard");
    };
    let named = |slot: ObjectId| play.object(slot).name.clone().unwrap();
    assert_eq!(
        [oust, low_plains, plains, low_island, island].map(named),
        [OUST, PLAINS, PLAINS, ISLAND, ISLAND]
    );
    let tray = format!(
        "tray #{island} #{low_island} #{plains} #{counterspell} #{low_plains} #{reprieve} \
         #{tithe} #{oust}"
    );
    assert_eq!(
        play.prompt(),
        format!(
            "pick exile-cost: Exile six other cards.\n\
             {tray}\n\
             picked 0 of 6\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&graveyard),
        )
    );
    // All but the lower Plains and the lower Island.
    let wanted = [oust, tithe, reprieve, plains, counterspell, island];
    let line = |slots: &[ObjectId]| {
        let slots: Vec<String> = slots.iter().map(|slot| format!(" #{slot}")).collect();
        slots.concat()
    };
    for (i, &slot) in wanted[..5].iter().enumerate() {
        let state = play.on(slot).unwrap();
        if i == 4 {
            assert_eq!(
                state,
                format!(
                    "pick exile-cost: Exile six other cards.\n\
                     {tray}\n\
                     picked 5 of 6:{}\n\
                     eligible {}\n\
                     selected {}\n\
                     confirm(Submit 5)\n",
                    line(&wanted[..5]),
                    cards(&[low_plains, low_island, island]),
                    cards(&wanted[..5]),
                )
            );
        }
    }
    let six = format!(
        "pick exile-cost: Exile six other cards.\n\
         {tray}\n\
         picked 6 of 6:{}\n\
         selected {}\n\
         confirm[Submit 6]*\n",
        line(&wanted),
        cards(&wanted),
    );
    assert_eq!(play.on(island).unwrap(), six);
    assert_eq!(play.on(low_island).unwrap(), six, "a seventh is refused");
    // The engine offers the lowest of cards alike: its picks are these six.
    let mut picked = Play::scenario(&DELVE);
    picked.choose(|c| matches!(c, Candidate::Cast { .. }));
    for slot in [oust, tithe, reprieve, low_plains, counterspell, low_island] {
        pick(&mut picked, slot);
    }
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(play.game.replay().log.len(), picked.game.replay().log.len());
    assert_eq!(play.view.players.you.graveyard[..2], [plains, island]);
}

/// The human's Abhorrent Oculus as the opponent's turn begins, over a Snapcaster Mage and a
/// Counterspell.
const DREAD: Scenario = Scenario::new("manifest dread")
    .at(3, P0, TurnStep::End)
    .permanents(P0, &[Permanent::new(ABHORRENT_OCULUS)])
    .library(P0, &[SNAPCASTER_MAGE, COUNTERSPELL, ISLAND, PLAINS])
    .library(P1, &[PLAINS; 6]);

/// A manifest dread lays out the two cards looked at; one is picked and its button manifests
/// it.
#[test]
fn a_manifest_dread_picks_one_of_the_two_cards_looked_at() {
    let mut play = Play::scenario(&DREAD);
    pass(&mut play);
    assert!(play.decision().prompt.is(Purpose::ManifestDread));
    let snapcaster = mine(&play, SNAPCASTER_MAGE, ZoneName::Library)[0];
    let counterspell = mine(&play, COUNTERSPELL, ZoneName::Library)[0];
    assert_eq!(
        play.prompt(),
        format!(
            "pick manifest-dread: Manifest one of them.\n\
             tray #{snapcaster} #{counterspell}\n\
             picked 0 of 1\n\
             eligible {}\n\
             confirm(Submit 0)\n",
            cards(&[snapcaster, counterspell]),
        )
    );
    assert_eq!(
        play.on(counterspell).unwrap(),
        format!(
            "pick manifest-dread: Manifest one of them.\n\
             tray #{snapcaster} #{counterspell}\n\
             picked 1 of 1: #{counterspell}\n\
             eligible #{snapcaster}\n\
             selected #{counterspell}\n\
             confirm[Submit 1]*\n"
        )
    );
    let mut picked = Play::scenario(&DREAD);
    pass(&mut picked);
    pick(&mut picked, counterspell);
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    assert_eq!(
        mine(&play, SNAPCASTER_MAGE, ZoneName::Graveyard),
        [snapcaster]
    );
}

/// A game of Islands and Counterspells, mulliganed twice and then kept, to the two cards
/// put on the bottom.
fn to_the_bottom() -> Play {
    let deck = vec![(30, ISLAND.to_owned()), (30, COUNTERSPELL.to_owned())];
    let start = StartGame {
        seed: 7,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::SMART,
    };
    let mut play = Play::seeded(&start);
    let mut mulligans = 0;
    while !play.decision().prompt.is(Purpose::PutOnBottom) {
        if play.decision().prompt.is(Purpose::Mulligan) && mulligans < 2 {
            mulligans += 1;
            play.choose(|c| *c == Candidate::Mulligan);
        } else if play.decision().prompt.is(Purpose::Mulligan) {
            play.choose(|c| *c == Candidate::Keep);
        } else {
            play.choose(|_| true);
        }
    }
    play
}

/// The cards put on the bottom after a mulligan are picked from the hand and sent together
/// by the button, in the order they were picked.
#[test]
fn a_mulligan_puts_its_cards_on_the_bottom_by_one_button() {
    let mut play = to_the_bottom();
    let hand: Vec<ObjectId> = (play.view.objects.iter())
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
        .map(|o| o.slot)
        .collect();
    assert_eq!(hand.len(), 7);
    assert_eq!(
        play.prompt(),
        format!(
            "pick put-on-bottom: Return 2 Cards\n\
             picked 0 of 2\n\
             eligible {}\n\
             confirm(Done)\n",
            cards(&hand),
        )
    );
    let island = *mine(&play, ISLAND, ZoneName::Hand).last().unwrap();
    let counterspell = *mine(&play, COUNTERSPELL, ZoneName::Hand).last().unwrap();
    play.on(island);
    assert_eq!(
        play.on(counterspell).unwrap(),
        format!(
            "pick put-on-bottom: Return 2 Cards\n\
             picked 2 of 2: #{island} #{counterspell}\n\
             selected {}\n\
             confirm[Done]*\n",
            cards(&[island, counterspell]),
        )
    );
    // The engine offers one card of each name: an Island, then a Counterspell.
    let mut picked = to_the_bottom();
    for name in [ISLAND, COUNTERSPELL] {
        let offered = (picked.decision().candidates.iter())
            .find_map(|c| match c.option {
                Candidate::Object { slot } => {
                    (picked.object(slot).name.as_deref() == Some(name)).then_some(slot)
                }
                _ => None,
            })
            .expect("a card of the name is offered");
        pick(&mut picked, offered);
    }
    assert_eq!(play.press(ButtonId::Confirm), None);
    assert_eq!(play.game.state(), picked.game.state());
    let hand = |play: &Play| {
        (play.view.objects.iter())
            .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
            .count()
    };
    assert_eq!(hand(&play), hand(&picked));
}
