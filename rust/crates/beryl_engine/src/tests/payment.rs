//! The mana solver (architecture §10, decision D7): payability, plans, and which payments are
//! decisions.

use super::cast::{Game, cast};
use crate::card::{ManaColor, ManaCost};
use crate::mana::Units;
use crate::state::{ObjectFlags, Zone};
use crate::{DecisionKind, Opt, Payment, Profile};

fn tapped(g: &Game, slot: u8) -> bool {
    g.s.object(slot).flags.has(ObjectFlags::TAPPED)
}

#[test]
fn colored_symbols_need_units_of_their_color() {
    let mut duals = Units::default();
    duals.add(0b11, 2);
    let w_u = ManaCost::new(0, [1, 1, 0, 0, 0, 0]);
    let w_w_u = ManaCost::new(0, [2, 1, 0, 0, 0, 0]);
    let one_w = ManaCost::new(1, [1, 0, 0, 0, 0, 0]);
    assert!(duals.pays(w_u));
    assert!(!duals.pays(w_w_u), "three symbols, two units");
    assert!(duals.pays(one_w));
    let mut crypt = Units::default();
    crypt.add(1 << ManaColor::Colorless as u8, 2);
    assert!(crypt.pays(ManaCost::generic(2)));
    assert!(
        !crypt.pays(ManaCost::new(1, [1, 0, 0, 0, 0, 0])),
        "{{C}} can't pay {{W}}"
    );
    // CR 107.4c: {C} in a cost needs colorless mana.
    let c = ManaCost::new(0, [0, 0, 0, 0, 0, 1]);
    assert!(crypt.pays(c));
    assert!(!duals.pays(c));
}

#[test]
fn plans_that_differ_only_in_which_lands_tap_are_not_a_decision() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let lands = [
        g.give(g.me, "Test Land", Zone::Battlefield),
        g.give(g.me, "Test Land", Zone::Battlefield),
        g.give(g.me, "Test Land", Zone::Battlefield),
    ];
    g.pick(cast(bear));
    assert_eq!(
        g.s.object(bear).zone,
        Zone::Stack,
        "paid without a question"
    );
    assert_eq!(lands.iter().filter(|&&l| tapped(&g, l)).count(), 2);
}

#[test]
fn plans_that_leave_different_mana_floating_are_a_decision() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let crypt = g.give(g.me, "Test Crypt", Zone::Battlefield);
    g.pick(cast(bear));
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::PaymentPlan);
    // Two lands, leaving nothing floating; or a land and the Crypt, leaving {C}.
    assert_eq!(d.len(), 2);
    g.pick(Opt::Plan(1));
    assert!(tapped(&g, crypt));
    assert_eq!(g.s.player(g.me).mana, [0, 0, 0, 0, 0, 1]);
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
}

/// A {1}{W} spell with a Plains, a Strip Mine and a W/U dual: the Plains pays {W}, and the
/// {1} comes from the Strip or the dual.
fn strip_or_dual(payment: Payment) -> (bool, bool) {
    let mut g = Game::new(Profile {
        payment,
        ..Profile::EXACT
    });
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let strip = g.give(g.me, "Test Strip", Zone::Battlefield);
    let dual = g.give(g.me, "Test Dual", Zone::Battlefield);
    g.pick(cast(bear));
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
    (tapped(&g, strip), tapped(&g, dual))
}

#[test]
fn the_exact_payment_keeps_the_most_costs_payable() {
    // Tapping the Strip would cost its own ability; the dual has no other use.
    assert_eq!(strip_or_dual(Payment::KeepMostPayable), (false, true));
}

#[test]
fn the_greedy_payment_taps_the_least_flexible_source() {
    // The Strip makes one kind of mana, the dual two.
    assert_eq!(strip_or_dual(Payment::Greedy), (true, false));
}

#[test]
fn manual_payment_makes_every_tap_a_decision() {
    let mut g = Game::new(Profile {
        payment: Payment::Manual,
        ..Profile::EXACT
    });
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let land = g.give(g.me, "Test Land", Zone::Battlefield);
    let island = g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.pick(cast(bear));
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::ManaPayment);
    // The two Islands are one candidate; tapping one first still leaves {W} payable.
    assert_eq!(d.len(), 2);
    g.pick(Opt::Mana {
        slot: island,
        color: ManaColor::Blue as u8,
    });
    // Only the Plains can pay the {W} left: no question.
    assert!(tapped(&g, land));
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
}

#[test]
fn manual_payment_offers_no_tap_that_strands_the_rest() {
    let mut g = Game::new(Profile {
        payment: Payment::Manual,
        ..Profile::EXACT
    });
    let raveler = g.give(g.me, "Test Raveler", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    let dual = g.give(g.me, "Test Dual", Zone::Battlefield);
    g.pick(cast(raveler));
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::ManaPayment);
    // {1}{W}{U} from a Plains, an Island and a dual: whichever color the dual pays first,
    // the other two lands pay the rest, so both its colors are offered.
    let dual_options = (0..d.len())
        .filter(|&i| {
            matches!(d.candidates.option(crate::Choice(i as u8)), Some(Opt::Mana { slot, .. }) if slot == dual)
        })
        .count();
    assert_eq!(dual_options, 2);
}
