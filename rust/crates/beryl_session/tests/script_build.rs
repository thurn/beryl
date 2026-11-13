//! The scripts the session builds (plan K23, K24), a hand-written expectation per verb, from
//! both seats: small scripted positions played through the engine, each seat's views fed to
//! its screen, the script between two of them read as text.

mod common;

use beryl_cards::decks::WU_CONTROL;
use beryl_cards::{CARDS, names};
use beryl_engine::Seat;
use beryl_session::protocol::{PlayerView, Why, ZoneName};
use beryl_session::script::{Act, Script, Step, Subject, Table, failure, replay};
use beryl_session::{CandidateIndex, Game, SessionId, StartGame, Status, StopsSetting};
use common::{script, scripts, text};

/// Prints every script of a scenario, to read what a change does to them:
/// `SCRIPT_DUMP="card|scenario" cargo test -p beryl_session --test script_build dump --
/// --ignored --nocapture`.
#[test]
#[ignore = "prints the scripts of the scenario SCRIPT_DUMP names"]
fn dump() {
    let which = std::env::var("SCRIPT_DUMP").expect("SCRIPT_DUMP=card|scenario");
    let (card, name) = which.split_once('|').expect("card|scenario");
    for (n, [first, second]) in scripts(card, name).into_iter().enumerate() {
        println!("== {n}\n-- first\n{first}-- second\n{second}");
    }
}

#[test]
fn a_seats_first_script_starts_its_screen_over() {
    let [first, second] = script(
        names::MANA_CRYPT,
        "Mana Crypt's flip lost: 3 damage to its controller",
        0,
    );
    // The upkeep trigger is on the stack as the scenario begins: each seat numbers it, and
    // shows at once the line from it to its source (`point`).
    assert_eq!(first, "reset a1\n+point source a1 > #1\n");
    assert_eq!(second, "reset a1\n+point source a1 > #1\n");
}

/// `fly`, `tap`, `streak` of mana, `point`, `dwell` and `vanish`: a spell is cast, and a
/// counterspell takes it off the stack and follows it. The counterspell's arrow begins as it
/// goes on the stack and ends in a beat of its own before its target leaves.
#[test]
fn a_spell_flies_to_the_stack_and_a_counterspell_takes_it_off() {
    let name = "Counterspell counters a spell into its owner's graveyard";
    // The second player casts Portable Hole, its Plains sending its mana into the spell.
    let [first, second] = script(names::COUNTERSPELL, name, 1);
    assert_eq!(
        first,
        text(&[
            "fly #257 (Portable Hole) hand>stack cast <- opponent",
            "tap #259 (Plains) <- opponent",
            "& streak #259 > #257 white 1 <- #259",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "fly #257 (Portable Hole) hand>stack cast <- you",
            "tap #259 (Plains) <- you",
            "& streak #259 > #257 white 1 <- #259",
        ])
    );
    // The first answers. Their Islands tap together, each sending its mana into the spell; the
    // spell is read by the player who did not cast it; the countered spell and the
    // counterspell leave in one beat.
    let [first, second] = script(names::COUNTERSPELL, name, 2);
    assert_eq!(
        first,
        text(&[
            "fly #1 (Counterspell) hand>stack cast at #257 <- you",
            "& +point hostile-target #1 > #257",
            "tap #0 (Island) <- you",
            "& tap #2 (Island) <- you",
            "& streak #0 > #1 blue 1 <- #0",
            "& streak #2 > #1 blue 1 <- #2",
            "-point hostile-target #1 > #257",
            "vanish #257 (Portable Hole) stack>graveyard counter <- #1",
            "& vanish #1 (Counterspell) stack>graveyard resolve <- #1",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "fly #1 (Counterspell) hand>stack cast at #257 <- opponent",
            "& +point hostile-target #1 > #257",
            "tap #0 (Island) <- opponent",
            "& tap #2 (Island) <- opponent",
            "& streak #0 > #1 blue 1 <- #0",
            "& streak #2 > #1 blue 1 <- #2",
            "dwell #1",
            "-point hostile-target #1 > #257",
            "vanish #257 (Portable Hole) stack>graveyard counter <- #1",
            "& vanish #1 (Counterspell) stack>graveyard resolve <- #1",
        ])
    );
}

/// `set`, `emerge`, `point` at a source, `appear` and `count`: an attacker with vigilance is
/// declared, its trigger lifts out of it with the line that joins the two, and the token the
/// trigger makes is there, which raises the attacker's power.
#[test]
fn an_attack_trigger_emerges_and_its_token_appears() {
    let name = "Adeline's Human arrives attacking";
    for (n, lines) in [
        (
            4,
            vec!["set #0 (Adeline, Resplendent Cathar) battlefield 2/4 attacking"],
        ),
        (
            7,
            vec![
                "appear #512 (Human Token) nowhere>battlefield created <- a1",
                "-point source a1 > #0",
                "vanish a1 stack>gone resolve <- a1",
                "count #0 (Adeline, Resplendent Cathar) pt 3/4",
            ],
        ),
    ] {
        let [first, second] = script(names::ADELINE_RESPLENDENT_CATHAR, name, n);
        assert_eq!(first, text(&lines), "script {n}");
        assert_eq!(second, text(&lines), "script {n}");
    }
    // The engine says a triggered ability with no target arrived on the stack, put there by
    // its controller.
    let [first, second] = script(names::ADELINE_RESPLENDENT_CATHAR, name, 5);
    assert_eq!(
        first,
        text(&[
            "emerge a1 (Adeline, Resplendent Cathar, triggered) you from #0 <- you",
            "& +point source a1 > #0",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "emerge a1 (Adeline, Resplendent Cathar, triggered) opponent from #0 <- opponent",
            "& +point source a1 > #0",
        ])
    );
}

/// `strike`: the attackers deal their combat damage at once, each with the life it takes.
#[test]
fn combat_damage_strikes_and_counts_life() {
    let [first, second] = script(
        names::ADELINE_RESPLENDENT_CATHAR,
        "Adeline's Human arrives attacking",
        12,
    );
    assert_eq!(
        first,
        text(&[
            "set turn 1 you declareBlockers",
            "set turn 1 you combatDamage",
            "strike #0 > opponent 3",
            "& count opponent life 17",
            "& strike #512 > opponent 1",
            "& count opponent life 16",
            "set turn 1 you endOfCombat",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "set turn 1 opponent declareBlockers",
            "set turn 1 opponent combatDamage",
            "strike #0 > you 3",
            "& count you life 17",
            "& strike #512 > you 1",
            "& count you life 16",
            "set turn 1 opponent endOfCombat",
        ])
    );
}

/// `announce` and `streak`: a coin a permanent's trigger flips, which names the permanent and
/// is the trigger's doing, and the damage the trigger
/// then deals, from the ability to the player, with the life it takes.
#[test]
fn a_coin_is_announced_and_noncombat_damage_streaks() {
    let [first, second] = script(
        names::MANA_CRYPT,
        "Mana Crypt's flip lost: 3 damage to its controller",
        2,
    );
    assert_eq!(
        first,
        text(&[
            "announce coin you lost by #1 <- a1",
            "streak a1 > you damage 3 <- a1",
            "& count you life 17 <- a1",
            "-point source a1 > #1",
            "vanish a1 stack>gone resolve <- a1",
            "set turn 3 you draw",
            "fly #0 (Island) library>hand draw",
            "set turn 3 you precombatMain",
        ])
    );
    // The opponent's draw has no name.
    assert_eq!(
        second,
        text(&[
            "announce coin opponent lost by #1 <- a1",
            "streak a1 > opponent damage 3 <- a1",
            "& count opponent life 17 <- a1",
            "-point source a1 > #1",
            "vanish a1 stack>gone resolve <- a1",
            "set turn 3 opponent draw",
            "fly #0 library>hand draw",
            "set turn 3 opponent precombatMain",
        ])
    );
}

/// `emerge` and `count`, in Arena's order: a loyalty ability is on the stack before its cost
/// shows on the planeswalker, though the cost is paid first. The attacker it exiles, which
/// attacks the player and so has no line, loses the ability's arrow in a beat before it goes. Then the turn passes:
/// `set`, the banner, and the lands untapping together.
#[test]
fn a_loyalty_cost_counts_after_its_ability_emerges() {
    let [first, second] = script(
        names::THE_WANDERING_EMPEROR,
        "flashed in during combat, \u{2212}2 exiles the tapped attacker",
        4,
    );
    assert_eq!(
        first,
        text(&[
            "emerge a1 (The Wandering Emperor, activated) you from #1 at #257 <- you",
            "& +point source a1 > #1",
            "& +point hostile-target a1 > #257",
            "count #1 (The Wandering Emperor) loyalty 1 <- you",
            "-point hostile-target a1 > #257",
            "vanish #257 (Snapcaster Mage) battlefield>exile exile <- a1",
            "count you life 22 <- a1",
            "-point source a1 > #1",
            "vanish a1 stack>gone resolve <- a1",
            "set turn 2 opponent declareBlockers",
            "set turn 2 opponent combatDamage",
            "set turn 2 opponent endOfCombat",
            "set turn 2 opponent postcombatMain",
            "set turn 2 opponent end",
            "set turn 2 opponent cleanup",
            "set turn 3 you untap",
            "& announce turn you",
            "untap #0 (Plains)",
            "& untap #2 (Plains)",
            "& untap #3 (Plains)",
            "& untap #4 (Plains)",
            "set turn 3 you upkeep",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "emerge a1 (The Wandering Emperor, activated) opponent from #1 at #257 <- opponent",
            "& +point source a1 > #1",
            "& +point hostile-target a1 > #257",
            "count #1 (The Wandering Emperor) loyalty 1 <- opponent",
            "dwell a1",
            "-point hostile-target a1 > #257",
            "vanish #257 (Snapcaster Mage) battlefield>exile exile <- a1",
            "count opponent life 22 <- a1",
            "-point source a1 > #1",
            "vanish a1 stack>gone resolve <- a1",
            "set turn 2 you declareBlockers",
            "set turn 2 you combatDamage",
            "set turn 2 you endOfCombat",
            "set turn 2 you postcombatMain",
            "set turn 2 you end",
            "set turn 2 you cleanup",
            "set turn 3 opponent untap",
            "& announce turn opponent",
            "untap #0 (Plains)",
            "& untap #2 (Plains)",
            "& untap #3 (Plains)",
            "& untap #4 (Plains)",
            "set turn 3 opponent upkeep",
        ])
    );
}

/// `count`: a counter put on a permanent by its own trigger.
#[test]
fn a_counter_counts() {
    let [first, second] = script(
        names::STAFF_OF_THE_STORYTELLER,
        "its own Spirit and a Samurai each add a story counter, spent to draw",
        4,
    );
    let lines = [
        "count #2 (Staff of the Storyteller) story 1 <- a2",
        "-point source a2 > #2",
        "vanish a2 stack>gone resolve <- a2",
    ];
    assert_eq!(first, text(&lines));
    assert_eq!(second, text(&lines));
}

/// `count`: a +1/+1 counter and the power and toughness it gives its creature are one step.
/// A step of its own for the numbers would follow the counter's if its record lacked them.
#[test]
fn a_plus_one_counter_counts_with_the_stats_it_changes() {
    let [first, second] = script(
        names::THE_WANDERING_EMPEROR,
        "+1 grows a creature and gives it first strike",
        3,
    );
    let lines = [
        "count #256 (Snapcaster Mage) plusOne 1 <- a1",
        "-point source a1 > #0",
        "& -point hostile-target a1 > #256",
        "vanish a1 stack>gone resolve <- a1",
    ];
    assert_eq!(first, text(&lines));
    assert_eq!(second, text(&lines));
}

/// `fly` within the battlefield: a permanent changes sides, the arrow at it ended first and
/// not drawn again by the ability that moved it.
#[test]
fn a_stolen_permanent_flies_to_its_new_side() {
    let [first, second] = script(
        names::THIEVING_SKYDIVER,
        "Thieving Skydiver kicked steals Mana Crypt over a Map",
        10,
    );
    assert_eq!(
        first,
        text(&[
            "-point hostile-target a1 > #256",
            "fly #256 (Mana Crypt) battlefield>battlefield you effect <- a1",
            "-point source a1 > #0",
            "vanish a1 stack>gone resolve <- a1",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "-point hostile-target a1 > #256",
            "fly #256 (Mana Crypt) battlefield>battlefield opponent effect <- a1",
            "-point source a1 > #0",
            "vanish a1 stack>gone resolve <- a1",
        ])
    );
}

/// `fly` onto a permanent: an Aura resolves onto the creature it is attached to, the arrow at
/// that creature ended first, and what it changes of the creature shows after it lands.
#[test]
fn an_aura_flies_onto_its_permanent() {
    let [first, second] = script(
        names::SWIFT_RECONFIGURATION,
        "a creature becomes a noncreature Vehicle that can't crew alone",
        2,
    );
    // Cast with flash, it resolves at once: no one can respond.
    let cast = |who: &str| {
        [
            format!("fly #0 (Swift Reconfiguration) hand>stack cast at #259 <- {who}"),
            "& +point hostile-target #0 > #259".to_owned(),
            format!("tap #1 (Plains) <- {who}"),
            "& streak #1 > #0 white 1 <- #1".to_owned(),
        ]
    };
    let lands = [
        "-point hostile-target #0 > #259",
        "fly #0 (Swift Reconfiguration) stack>battlefield on #259 resolve <- #0",
        "count #259 (Snapcaster Mage) pt",
    ];
    let mine: Vec<String> = cast("you")
        .into_iter()
        .chain(lands.map(String::from))
        .collect();
    let theirs: Vec<String> = cast("opponent")
        .into_iter()
        .chain(["dwell #0".to_owned()])
        .chain(lands.map(String::from))
        .collect();
    assert_eq!(first, text(&mine));
    assert_eq!(second, text(&theirs));
}

/// `vanish` in a batch: a sweeper's creatures leave in one beat, the token with no record
/// and the face-down one under its name.
#[test]
fn a_sweepers_creatures_vanish_together() {
    let [first, second] = script(
        names::DAMN,
        "Damn overloaded takes tokens and face-down creatures at once",
        2,
    );
    let lines = [
        "vanish #0 (Adeline, Resplendent Cathar) battlefield>graveyard destroy <- #1",
        "& vanish #512 battlefield>gone destroy <- #1",
        "& vanish #257 (Elite Spellbinder) battlefield>graveyard destroy <- #1",
        "& vanish #256 (Snapcaster Mage) battlefield>graveyard destroy <- #1",
        "vanish #1 (Damn) stack>graveyard resolve <- #1",
    ];
    assert_eq!(first, text(&lines));
    assert_eq!(second, text(&lines));
}

/// `fly` out of a library and a hand, and `lift`: cards drawn together fly in one beat, face
/// down for the seat that does not see them, and the cards discarded after them fly to the
/// graveyard in the next. The look the opponent's seat saw lifted ends before they move.
#[test]
fn cards_drawn_and_discarded_fly_from_both_seats() {
    let [first, second] = script(
        names::THIRST_FOR_DISCOVERY,
        "Thirst for Discovery: discarding two cards with a basic land in hand",
        5,
    );
    let lines = [
        "fly #6 (Plains) library>hand draw <- #2",
        "& fly #7 (Portable Hole) library>hand draw <- #2",
        "& fly #3 (Counterspell) library>hand draw <- #2",
        "fly #7 (Portable Hole) hand>graveyard discard <- #2",
        "& fly #6 (Plains) hand>graveyard discard <- #2",
        "vanish #2 (Thirst for Discovery) stack>graveyard resolve <- #2",
    ];
    assert!(first.starts_with(&text(&lines)), "{first}");
    // The opponent's seat knows only the card it was shown, until the others are discarded.
    let lines = [
        "-lift opponent library all",
        "fly #6 library>hand draw <- #2",
        "& fly #7 (Portable Hole) library>hand draw <- #2",
        "& fly #3 library>hand draw <- #2",
        "fly #7 (Portable Hole) hand>graveyard discard <- #2",
        "& fly #6 (Plains) hand>graveyard discard <- #2",
        "vanish #2 (Thirst for Discovery) stack>graveyard resolve <- #2",
    ];
    assert!(second.starts_with(&text(&lines)), "{second}");
}

/// A spell returned to its owner's hand: its owner's seat sees it `fly` there, and the seat
/// across from it sees it `vanish` on the stack (UI Conventions, "An opponent's spell
/// returned to hand", "Your own spell returned to hand"). The card drawn after flies to its
/// hand, unseen by the other seat.
#[test]
fn a_returned_spell_flies_for_its_owner_and_vanishes_for_the_other_seat() {
    let name = "Reprieve returns a spell with X to its owner's hand and draws";
    let [first, second] = script(names::REPRIEVE, name, 3);
    assert_eq!(
        first,
        text(&[
            "fly #258 (Reprieve) hand>stack cast at #1 <- opponent",
            "& +point hostile-target #258 > #1",
            "tap #256 (Plains) <- opponent",
            "& tap #257 (Island) <- opponent",
            "& streak #256 > #258 white 1 <- #256",
            "& streak #257 > #258 blue 1 <- #257",
            "dwell #258",
            "-point hostile-target #258 > #1",
            "fly #1 (Thieving Skydiver) stack>hand effect <- #258",
            "fly #259 library>hand draw <- #258",
            "vanish #258 (Reprieve) stack>graveyard resolve <- #258",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "fly #258 (Reprieve) hand>stack cast at #1 <- you",
            "& +point hostile-target #258 > #1",
            "tap #256 (Plains) <- you",
            "& tap #257 (Island) <- you",
            "& streak #256 > #258 white 1 <- #256",
            "& streak #257 > #258 blue 1 <- #257",
            "-point hostile-target #258 > #1",
            "vanish #1 (Thieving Skydiver) stack>hand effect <- #258",
            "fly #259 (Plains) library>hand draw <- #258",
            "vanish #258 (Reprieve) stack>graveyard resolve <- #258",
        ])
    );
}

/// `vanish` into exile: the permanent goes where it lay, for both seats, and the ability
/// that took it after it.
#[test]
fn an_exiled_permanent_vanishes() {
    let name = "Portable Hole exiles until it leaves the battlefield";
    let [first, second] = script(names::PORTABLE_HOLE, name, 3);
    let lines = [
        "-point hostile-target a1 > #261",
        "vanish #261 (Snapcaster Mage) battlefield>exile exile <- a1",
        "-point source a1 > #2",
        "vanish a1 stack>gone resolve <- a1",
    ];
    assert_eq!(first, text(&lines));
    assert_eq!(second, text(&lines));
}

/// `vanish` into a library: the place the card takes in it, and no name there.
#[test]
fn a_permanent_put_into_a_library_takes_its_place() {
    let [first, second] = script(
        names::OUST,
        "Oust puts the creature second from the top and its controller gains 3",
        1,
    );
    assert_eq!(
        first,
        text(&[
            "fly #1 (Oust) hand>stack cast at #256 <- you",
            "& +point hostile-target #1 > #256",
            "tap #3 (Plains) <- you",
            "& streak #3 > #1 white 1 <- #3",
            "-point hostile-target #1 > #256",
            "vanish #256 (Elite Spellbinder) battlefield>library @1 effect <- #1",
            "count opponent life 23 <- #1",
            "vanish #1 (Oust) stack>graveyard resolve <- #1",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "fly #1 (Oust) hand>stack cast at #256 <- opponent",
            "& +point hostile-target #1 > #256",
            "tap #3 (Plains) <- opponent",
            "& streak #3 > #1 white 1 <- #3",
            "dwell #1",
            "-point hostile-target #1 > #256",
            "vanish #256 (Elite Spellbinder) battlefield>library @1 effect <- #1",
            "count you life 23 <- #1",
            "vanish #1 (Oust) stack>graveyard resolve <- #1",
        ])
    );
}

/// `flip` and `lift`: a manifested card comes onto the battlefield face down, named only for
/// its controller, and turns face up where it lies. No event says that it turned: the step
/// closes the script. The opponent sees the two cards looked at come off the library while
/// the choice is made, and the one not manifested is then in the graveyard with no flight.
#[test]
fn a_face_down_permanent_flips() {
    let name = "A manifested creature turns face up for its mana cost, entering no more";
    // The player choosing sees both cards' faces; the other sees them lifted.
    let [first, second] = script(names::ABHORRENT_OCULUS, name, 2);
    assert_eq!(
        first,
        text(&["flip #2 (Snapcaster Mage)", "& flip #4 (Island)"])
    );
    assert_eq!(second, "+lift opponent library 2\n");
    let [first, second] = script(names::ABHORRENT_OCULUS, name, 3);
    assert_eq!(
        first,
        text(&[
            "fly #2 (Snapcaster Mage, face down) library>battlefield effect <- a1",
            "fly #4 (Island) library>graveyard effect <- a1",
            "-point source a1 > #0",
            "vanish a1 stack>gone resolve <- a1",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "-lift opponent library 2",
            "fly #2 (face down) library>battlefield effect <- a1",
            "appear #4 (Island) library>graveyard effect <- a1",
            "-point source a1 > #0",
            "vanish a1 stack>gone resolve <- a1",
        ])
    );
    let [first, second] = script(names::ABHORRENT_OCULUS, name, 4);
    assert_eq!(
        first,
        text(&[
            "tap #1 (Island) <- you",
            "& tap #3 (Island) <- you",
            "set turn 2 opponent draw",
            "fly #256 library>hand draw",
            "set turn 2 opponent precombatMain",
            "flip #2 (Snapcaster Mage)",
        ])
    );
    assert_eq!(
        second,
        text(&[
            "tap #1 (Island) <- opponent",
            "& tap #3 (Island) <- opponent",
            "set turn 2 you draw",
            "fly #256 (Plains) library>hand draw",
            "set turn 2 you precombatMain",
            "flip #2 (Snapcaster Mage)",
        ])
    );
}

/// `set` of a library: both hands go into their libraries and each library is shuffled with
/// nothing to show, all in one beat as one instruction for each player is one event of the
/// rules, and each player's seven are drawn in a beat.
#[test]
fn a_shuffle_sets_the_library() {
    let [first, second] = script(names::TIMETWISTER, "Timetwister with empty graveyards", 1);
    let cast = |who: &str| {
        vec![
            format!("fly #1 (Timetwister) hand>stack cast <- {who}"),
            format!("tap #0 (Island) <- {who}"),
            format!("& tap #5 (Island) <- {who}"),
            format!("& tap #9 (Island) <- {who}"),
            "& streak #0 > #1 blue 1 <- #0".to_owned(),
            "& streak #5 > #1 blue 1 <- #5".to_owned(),
            "& streak #9 > #1 blue 1 <- #9".to_owned(),
        ]
    };
    let draws = |slots: [u16; 7], card: Option<&str>| -> Vec<String> {
        slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let join = if i == 0 { "" } else { "& " };
                let card = card.map_or_else(String::new, |card| format!(" ({card})"));
                format!("{join}fly #{slot}{card} library>hand draw <- #1")
            })
            .collect()
    };
    let wheel = [
        "fly #256 hand>library top effect <- #1",
        "& fly #257 hand>library top effect <- #1",
        "& fly #2 hand>library top effect <- #1",
    ]
    .map(str::to_owned);
    let (own, theirs) = ([3, 11, 8, 4, 10, 7, 2], [263, 258, 261, 260, 256, 259, 257]);
    let mut expected = cast("you");
    expected.extend(wheel.clone());
    expected.push("& set you library 8 <- #1".to_owned());
    expected.push("& set opponent library 8 <- #1".to_owned());
    expected.extend(draws(own, Some("Plains")));
    expected.extend(draws(theirs, None));
    expected.push("vanish #1 (Timetwister) stack>graveyard resolve <- #1".to_owned());
    assert_eq!(first, text(&expected));
    let mut expected = cast("opponent");
    expected.push("dwell #1".to_owned());
    expected.extend(wheel);
    expected.push("& set opponent library 8 <- #1".to_owned());
    expected.push("& set you library 8 <- #1".to_owned());
    expected.extend(draws(own, None));
    expected.extend(draws(theirs, Some("Island")));
    expected.push("vanish #1 (Timetwister) stack>graveyard resolve <- #1".to_owned());
    assert_eq!(second, text(&expected));
}

/// `appear` of an opening hand and `vanish` of a mulligan's: before the first turn a hand is
/// dealt with no event, its cards seen where they come to lie (Arena's case "opening hand"),
/// both seats' at once. A mulligan's hand goes back first, in a beat of its own, every card of
/// it, those whose slots the shuffled library deals out again among them; then the next is
/// dealt.
#[test]
fn an_opening_hand_appears_and_a_mulligans_goes_back_first() {
    let deck = || -> Vec<(u8, String)> {
        let cards = WU_CONTROL.iter();
        cards.map(|&(n, name)| (n, name.to_owned())).collect()
    };
    let start = StartGame {
        seed: 34,
        decks: [deck(), deck()],
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
    };
    let mut game = Game::new(&CARDS, &start, SessionId(1)).unwrap();
    let mut scripts = Vec::new();
    for answer in [Some("Mulligan"), None] {
        let mut last = None;
        while let Some(view) = game.next_view() {
            scripts.push(view.script.as_ref().map(ToString::to_string));
            last = Some(view);
        }
        let Some(answer) = answer else { break };
        let Some(Status::Deciding { decision }) = last.map(|v| v.status) else {
            panic!("no keep or mulligan");
        };
        let at = decision.candidates.iter().position(|c| c.label == answer);
        let at = u16::try_from(at.expect("a mulligan to take")).unwrap();
        let at = CandidateIndex(at);
        game.choose(game.answer(decision.id, at).unwrap());
    }
    let dealt = [
        "appear #33 (Plains) library>hand draw",
        "& appear #34 (No More Lies) library>hand draw",
        "& appear #35 (Jace, Wielder of Mysteries) library>hand draw",
        "& appear #36 (Subtlety) library>hand draw",
        "& appear #37 (Abhorrent Oculus) library>hand draw",
        "& appear #38 (Tundra) library>hand draw",
        "& appear #39 (Island) library>hand draw",
        "& appear #289 library>hand draw",
        "& appear #290 library>hand draw",
        "& appear #291 library>hand draw",
        "& appear #292 library>hand draw",
        "& appear #293 library>hand draw",
        "& appear #294 library>hand draw",
        "& appear #295 library>hand draw",
        "& set turn 0 opponent untap",
        "+lift opponent hand",
    ];
    assert_eq!(scripts[1].as_deref(), Some(text(&dealt).as_str()));
    let again = [
        "vanish #33 hand>library top effect",
        "& vanish #34 hand>library top effect",
        "& vanish #35 hand>library @21 effect",
        "& vanish #36 hand>library @31 effect",
        "& vanish #37 hand>library @6 effect",
        "& vanish #38 hand>library @32 effect",
        "& vanish #39 hand>library top effect",
        "appear #4 (Stock Up) library>hand draw",
        "& appear #12 (Oust) library>hand draw",
        "& appear #28 (Island) library>hand draw",
        "& appear #30 (Seachrome Coast) library>hand draw",
        "& appear #33 (Plains) library>hand draw",
        "& appear #34 (Teferi, Hero of Dominaria) library>hand draw",
        "& appear #39 (Counterspell) library>hand draw",
        "& set you library 33",
    ];
    assert_eq!(
        scripts.last().unwrap().as_deref(),
        Some(text(&again).as_str())
    );
}

/// The views of a seeded game of ten of the human's answers, each the first candidate, with
/// the last of them then taken back.
fn game_views() -> Vec<PlayerView> {
    let deck = || -> Vec<(u8, String)> {
        let cards = WU_CONTROL.iter();
        cards.map(|&(n, name)| (n, name.to_owned())).collect()
    };
    let start = StartGame {
        seed: 3,
        decks: [deck(), deck()],
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
    };
    let mut game = Game::new(&CARDS, &start, SessionId(1)).unwrap();
    let mut views = Vec::new();
    for _ in 0..10 {
        while let Some(view) = game.next_view() {
            views.push(view);
        }
        let Some(Status::Deciding { decision }) = views.last().map(|v| &v.status) else {
            break;
        };
        game.choose(game.answer(decision.id, CandidateIndex(0)).unwrap());
    }
    while let Some(view) = game.next_view() {
        views.push(view);
    }
    views.push(game.undo().expect("an answer to take back"));
    views
}

/// Every view a game sends has its seat's script; the first starts the screen over, and so
/// does a view that takes an answer back; and each of the others, replayed over the table of
/// the view before, gives its own view's table (the check every debug build runs as it
/// builds one).
#[test]
fn every_view_of_a_game_carries_a_script_that_replays_to_it() {
    let views = game_views();
    let script = |view: &PlayerView| view.script.clone().expect("a view's script");
    assert!(script(&views[0]).reset.is_some());
    let mut played = 0;
    for pair in views.windows(2) {
        let (before, view) = (&pair[0], &pair[1]);
        let script = script(view);
        assert_eq!(
            script.reset.is_some(),
            view.undone,
            "only an undo starts over"
        );
        played += script.beats.len();
        // The abilities' numbers are the scripts' own, so a table is carried forward only by
        // replay: here each script is checked from its view before, whose stack is empty.
        if before.stack.is_empty() && !view.undone {
            assert_eq!(failure(&Table::of(before), &script, view), None);
        }
    }
    assert!(
        played > 20,
        "the game's scripts say what happened: {played} beats"
    );
    // On the wire a view's script is its `script` field, and a view without one still reads.
    let last = views.last().unwrap();
    let mut wire = serde_json::to_value(last).unwrap();
    assert!(wire["script"]["reset"].is_object());
    wire.as_object_mut().unwrap().remove("script");
    let read: PlayerView = serde_json::from_value(wire).unwrap();
    assert_eq!(read.script, None);
    // Views are equal by what they show, their scripts aside.
    assert_eq!(&read, last);
}

/// A script that leaves a change out fails the replay check, which names the first part of
/// the table that differs and prints the script.
#[test]
fn a_script_that_leaves_a_change_out_fails_the_replay_check() {
    let views = game_views();
    let (before, view) = views
        .windows(2)
        .map(|pair| (&pair[0], &pair[1]))
        .find(|(before, view)| {
            before.stack.is_empty() && !view.undone && before.turn.step != view.turn.step
        })
        .expect("a step begins");
    let whole = view.script.clone().unwrap();
    assert_eq!(failure(&Table::of(before), &whole, view), None);
    // Without its `set turn` steps, the script leaves the table in the step before.
    let turn = |text: String| text.starts_with("set turn");
    let mut cut = whole.clone();
    for beat in &mut cut.beats {
        beat.retain(|step| !turn(step.to_string()));
    }
    let wrong = failure(&Table::of(before), &cut, view).expect("the check fails");
    assert!(
        wrong.starts_with("table.turn.") && wrong.contains("in the script\n"),
        "{wrong}"
    );
    // A step the table cannot take is named too: an ability that is not on the stack.
    let vanish = Step {
        act: Act::Vanish {
            subject: Subject::Ability { id: 1 },
            from: ZoneName::Stack,
            why: Why::Resolve,
            object: None,
            at: None,
        },
        cause: None,
    };
    let bad = Script {
        reset: None,
        beats: vec![vec![vanish]],
    };
    let error = replay(Table::of(before), &bad).unwrap_err();
    assert_eq!(
        error,
        "`vanish a1 stack>gone resolve`: no ability a1 is on the stack"
    );
}
