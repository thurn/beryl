//! The scripts of cards leaving and entering piles beyond a hand and the battlefield (plan
//! K24): a hand-written expectation for each, from both seats, beside script_build.rs's.

mod common;

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
            "vanish a1 stack>gone resolve".to_owned(),
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
            format!("fly #4 (Counterspell) graveyard>stack cast flashback at #261 <- {who}"),
            "& +point hostile-target #4 > #261".to_owned(),
            format!("tap #3 (Island) <- {who}"),
            format!("& streak #3 > #4 blue 1 <- {who}"),
            format!("& tap #5 (Island) <- {who}"),
            format!("& streak #5 > #4 blue 1 <- {who}"),
        ];
        if who == "opponent" {
            lines.push("dwell #4".to_owned());
        }
        lines.extend([
            "-point hostile-target #4 > #261".to_owned(),
            "vanish #261 (Timetwister) stack>graveyard counter <- #4".to_owned(),
            "& vanish #4 (Counterspell) stack>exile resolve".to_owned(),
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
            "fly #260 (Thieving Skydiver) exile>stack cast <- {who}"
        )];
        for (i, land) in [257, 258, 259, 261].into_iter().enumerate() {
            let and = if i == 0 { "" } else { "& " };
            lines.push(format!("{and}tap #{land} (Island) <- {who}"));
            lines.push(format!("& streak #{land} > #260 blue 1 <- {who}"));
        }
        text(&lines)
    };
    assert_eq!(first, cast("opponent"));
    assert_eq!(second, cast("you"));
}
