//! The scripts of cards leaving and entering piles beyond a hand and the battlefield, and of
//! cards revealed: a hand-written expectation for each, from both seats, beside
//! `script_build.rs`'s.

use crate::common;

use beryl_cards::names;
use common::{script, text};

/// `fly` of a mill: the cards go from the library to the graveyard face up, together, and the
/// draw that follows is a beat of its own.
#[test]
fn milled_cards_fly_to_the_graveyard_together() {
    let name = "+1 mills the target player two, then draws";
    let [first, second] = script(names::JACE_WIELDER_OF_MYSTERIES, name, 3);
    let lines = |drawn: &str, at: &str| {
        text(&[
            "fly #256 (Plains) library>graveyard effect <- a1".to_owned(),
            "& fly #258 (Plains) library>graveyard effect <- a1".to_owned(),
            format!("fly #2 {drawn}library>hand draw <- a1"),
            "-point source a1 > #0".to_owned(),
            format!("& -point hostile-target a1 > {at}"),
            "vanish a1 stack>gone resolve <- a1".to_owned(),
        ])
    };
    assert_eq!(first, lines("(Island) ", "opponent"));
    assert_eq!(second, lines("", "you"));
}

/// `fly` of a card cast from a graveyard: from its pile to the stack, as one cast from a hand.
#[test]
fn a_card_cast_with_flashback_flies_from_the_graveyard() {
    let name = "Snapcaster Mage: flashback a counterspell, then exile it";
    let [first, second] = script(names::SNAPCASTER_MAGE, name, 5);
    let cast = |who: &str| {
        let mut lines = vec![
            format!("fly #0 (Counterspell) graveyard>stack cast flashback at #257 <- {who}"),
            "& +point hostile-target #0 > #257".to_owned(),
            format!("tap #4 (Island) <- {who}"),
            format!("& tap #5 (Island) <- {who}"),
            "& streak #4 > #0 blue 1 <- #4".to_owned(),
            "& streak #5 > #0 blue 1 <- #5".to_owned(),
        ];
        if who == "opponent" {
            lines.push("dwell #0".to_owned());
        }
        lines.extend([
            "-point hostile-target #0 > #257".to_owned(),
            "vanish #257 (Timetwister) stack>graveyard counter <- #0".to_owned(),
            "& vanish #0 (Counterspell) stack>exile resolve <- #0".to_owned(),
        ]);
        text(&lines)
    };
    assert_eq!(first, cast("you"));
    assert_eq!(second, cast("opponent"));
}

/// `fly` of a card cast from exile: from the pile Elite Spellbinder put it in to the stack.
#[test]
fn a_card_cast_from_exile_flies_to_the_stack() {
    let name = "Spellbinder's card costs its owner {2} more";
    let [first, second] = script(names::ELITE_SPELLBINDER, name, 10);
    let cast = |who: &str| {
        let mut lines = vec![format!(
            "fly #260 (Portable Hole) exile>stack cast <- {who}"
        )];
        // The three lands tap together, and then each sends its mana, which is its own
        // ability's doing.
        let lands = [257, 259, 261];
        for (i, land) in lands.into_iter().enumerate() {
            let and = if i == 0 { "" } else { "& " };
            lines.push(format!("{and}tap #{land} (Plains) <- {who}"));
        }
        for land in lands {
            lines.push(format!("& streak #{land} > #260 white 1 <- #{land}"));
        }
        text(&lines)
    };
    assert_eq!(first, cast("opponent"));
    assert_eq!(second, cast("you"));
}

/// `showcase` of a card found by a search: it is held up where it lies, in the library, for
/// the opponent only, and then goes to the hand. The searcher already knows the card, so
/// their seat gets only its flight to the hand.
#[test]
fn a_card_found_and_revealed_is_held_up_before_it_goes_to_the_hand() {
    let [first, second] = script(names::TRINKET_MAGE, "Trinket Mage finds Mana Crypt", 6);
    let found = ["fly #5 (Mana Crypt) library>hand effect <- a1"];
    let held = ["showcase #5 (Mana Crypt) <- a1"];
    let own = [
        "set you library 5 <- a1",
        "-point source a1 > #4",
        "vanish a1 stack>gone resolve <- a1",
        // The searcher forgets the cards the shuffle hid again.
        "flip #2",
        "& flip #3",
        "& flip #7",
        "& flip #8",
        "& flip #9",
    ];
    let theirs = [
        "set opponent library 5 <- a1",
        "-point source a1 > #4",
        "vanish a1 stack>gone resolve <- a1",
    ];
    assert_eq!(first, text(&[&found[..], &own[..]].concat()));
    let look = ["-lift opponent library all"];
    assert_eq!(
        second,
        text(&[&look[..], &held[..], &found[..], &theirs[..]].concat())
    );
}

/// `showcase` of an explore's card: revealed on top of the library, it is held up for the
/// opponent only, before the land goes to the hand, and where a nonland stays on top it is
/// held up all the same.
#[test]
fn an_explored_card_is_held_up_whether_it_leaves_the_library_or_stays() {
    let name = "Get Lost on a creature gives two Maps; one explores a land into hand";
    let [theirs, own] = script(names::GET_LOST, name, 6);
    let held = "showcase #261 (Plains) <- a1\nfly #261 (Plains) library>hand effect <- a1\n";
    assert!(theirs.contains(held), "{theirs}");
    assert!(!own.contains("showcase"), "{own}");
    let name = "A Map explores a nonland into a counter, left on top";
    let rest = [
        "count #257 (Elite Spellbinder) plusOne 1 <- a1",
        "-point friendly-target a1 > #257",
        "vanish a1 stack>gone resolve <- a1",
    ];
    let held = text(&[&["showcase #261 (Oust) <- a1"][..], &rest[..]].concat());
    assert_eq!(script(names::GET_LOST, name, 4), [held, text(&rest)]);
}

/// `showcase` of a face-down permanent that leaves for a zone that shows no face: it is held
/// up as it goes, in the beat that takes it.
#[test]
fn a_face_down_permanent_put_into_a_library_is_held_up_as_it_goes() {
    let name = "A face-down permanent leaving the battlefield is revealed";
    let [first, second] = script(names::ABHORRENT_OCULUS, name, 7);
    let lines = |gains: &str| {
        text(&[
            "-point hostile-target #256 > #0".to_owned(),
            "vanish #0 (Snapcaster Mage) battlefield>library bottom effect <- #256".to_owned(),
            "& showcase #0 (Snapcaster Mage) <- #256".to_owned(),
            format!("count {gains} life 23 <- #256"),
            "vanish #256 (Oust) stack>graveyard resolve <- #256".to_owned(),
        ])
    };
    assert_eq!(first, lines("you"));
    assert_eq!(second, lines("opponent"));
}

/// No `showcase` of a card that only changes zone, however public the zone it goes to: a
/// milled card's flight shows it.
#[test]
fn a_card_that_is_not_revealed_is_not_held_up() {
    let name = "+1 mills the target player two, then draws";
    for milled in script(names::JACE_WIELDER_OF_MYSTERIES, name, 3) {
        assert!(!milled.contains("showcase"), "{milled}");
    }
}

/// `announce` of a top-or-bottom choice: when the opponent puts their spell on the top or the
/// bottom of their library, a pill names the end they chose as the card goes there. The seat
/// that chose sees no pill.
#[test]
fn the_opponents_choice_of_top_or_bottom_is_announced() {
    let cases = [
        (
            "Subtlety hard cast: the creature spell's owner puts it on top",
            7,
            "#261 (Elite Spellbinder)",
            "top",
            &[
                "-point source a1 > #0",
                "vanish a1 stack>gone resolve <- a1",
            ][..],
        ),
        (
            "Subtlety evoked: sacrificed, its trigger still tucks a planeswalker spell",
            10,
            "#258 (Teferi, Time Raveler)",
            "bottom",
            &["vanish a1 stack>gone resolve <- a1"][..],
        ),
    ];
    for (name, n, spell, end, after) in cases {
        let [first, second] = script(names::SUBTLETY, name, n);
        let lines = |told: bool| {
            let mut lines = vec![
                format!("-point hostile-target a1 > {}", &spell[..4]),
                format!("vanish {spell} stack>library {end} effect <- a1"),
            ];
            if told {
                lines.push(format!("& announce end opponent {end} <- a1"));
            }
            lines.extend(after.iter().map(|&line| line.to_owned()));
            text(&lines)
        };
        assert_eq!(first, lines(true), "{name}");
        assert_eq!(second, lines(false), "{name}");
    }
}
