//! The scripts of cards revealed (plan K24): a hand-written expectation of each `showcase`,
//! from both seats, beside `script_build.rs`'s.

mod common;

use beryl_cards::names;
use common::{script, text};

/// `showcase` of a card found by a search: it is held up where it lies, in the library, for
/// both seats, and then goes to the hand.
#[test]
fn a_card_found_and_revealed_is_held_up_before_it_goes_to_the_hand() {
    let [first, second] = script(names::TRINKET_MAGE, "Trinket Mage finds Mana Crypt", 6);
    let found = [
        "showcase #5 (Mana Crypt) <- a1",
        "fly #5 (Mana Crypt) library>hand effect <- a1",
    ];
    let own = [
        "set you library 5 <- a1",
        "-point source a1 > #4",
        "vanish a1 stack>gone resolve",
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
        "vanish a1 stack>gone resolve",
    ];
    assert_eq!(first, text(&[&found[..], &own[..]].concat()));
    let look = ["-lift opponent library all"];
    assert_eq!(second, text(&[&look[..], &found[..], &theirs[..]].concat()));
}

/// `showcase` of an explore's card: revealed on top of the library, it is held up before the
/// land goes to the hand, and where a nonland stays on top it is held up all the same.
#[test]
fn an_explored_card_is_held_up_whether_it_leaves_the_library_or_stays() {
    let name = "Get Lost on a creature gives two Maps; one explores a land into hand";
    for explored in script(names::GET_LOST, name, 6) {
        let held = "showcase #261 (Plains) <- a1\nfly #261 (Plains) library>hand effect <- a1\n";
        assert!(explored.contains(held), "{explored}");
    }
    let name = "A Map explores a nonland into a counter, left on top";
    let stays = text(&[
        "showcase #261 (Oust) <- a1",
        "count #257 (Elite Spellbinder) plusOne 1 <- a1",
        "-point friendly-target a1 > #257",
        "vanish a1 stack>gone resolve",
    ]);
    assert_eq!(script(names::GET_LOST, name, 4), [stays.clone(), stays]);
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
            "vanish #256 (Oust) stack>graveyard resolve".to_owned(),
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
