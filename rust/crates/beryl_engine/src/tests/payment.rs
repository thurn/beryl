//! The mana solver (architecture §10, decision D7): payability, plans, and which payments are
//! decisions.

use super::cast::{Game, cast};
use crate::bitboard::Bitboard;
use crate::card::{ManaColor, ManaCost};
use crate::event::Sink;
use crate::ids::Slot;
use crate::mana::{self, Units};
use crate::rng::SplitMix64;
use crate::state::{ManaPool, ObjectFlags, Zone};
use crate::{DecisionKind, Opt, Payment, Profile};

fn tapped(g: &Game, slot: Slot) -> bool {
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
    assert_eq!(g.s.player(g.me).mana.amounts(), [0, 0, 0, 0, 0, 1]);
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
        color: ManaColor::Blue,
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

#[test]
fn a_payment_never_taps_a_source_it_could_leave_untapped() {
    for payment in [Payment::KeepMostPayable, Payment::Greedy] {
        let mut g = Game::new(Profile {
            payment,
            ..Profile::EXACT
        });
        let overlord = g.give(g.me, "Test Overlord", Zone::Hand);
        let lands = [
            g.give(g.me, "Test Land", Zone::Battlefield),
            g.give(g.me, "Test Land", Zone::Battlefield),
            g.give(g.me, "Test Land", Zone::Battlefield),
        ];
        let crypt = g.give(g.me, "Test Crypt", Zone::Battlefield);
        // {3}{W}: the Crypt and two Plains pay it. Tapping the third Plains as well would
        // only float {C}, so it is no plan of its own.
        g.pick(cast(overlord));
        assert_eq!(g.s.object(overlord).zone, Zone::Stack, "{payment:?}");
        assert!(tapped(&g, crypt), "{payment:?}");
        assert_eq!(
            lands.iter().filter(|&&l| tapped(&g, l)).count(),
            2,
            "{payment:?}"
        );
        assert_eq!(g.s.player(g.me).mana, ManaPool::default(), "{payment:?}");
    }
}

/// The mana each test source can make: its kinds, and how many it makes.
fn makes(name: &str) -> (&'static [usize], u8) {
    const W: usize = ManaColor::White as usize;
    const U: usize = ManaColor::Blue as usize;
    const C: usize = ManaColor::Colorless as usize;
    match name {
        "Test Land" | "Test HQ" => (&[W], 1),
        "Test Island" => (&[U], 1),
        "Test Dual" => (&[W, U], 1),
        "Test Crypt" => (&[C], 2),
        "Test Strip" => (&[C], 1),
        "Test Treasure" => (&[W, U, 2, 3, 4], 1),
        _ => unreachable!("{name} is no source"),
    }
}

/// Whether `cost` can be paid from `pool` and some of `sources`, each tapped for one of its
/// kinds: every way of tapping them, tried.
fn brute_force_pays(cost: ManaCost, pool: [u8; 6], sources: &[&str]) -> bool {
    let Some((first, rest)) = sources.split_first() else {
        let symbols_covered = (0..6).all(|k| pool[k] >= cost.symbols[k]);
        let total: u32 = pool.iter().map(|&n| u32::from(n)).sum();
        return symbols_covered && total >= cost.total(0);
    };
    let (kinds, amount) = makes(first);
    brute_force_pays(cost, pool, rest)
        || kinds.iter().any(|&k| {
            let mut more = pool;
            more[k] += amount;
            brute_force_pays(cost, more, rest)
        })
}

#[test]
fn the_solver_pays_exactly_the_costs_brute_force_can_pay() {
    const SOURCES: [&str; 7] = [
        "Test Land",
        "Test Island",
        "Test Dual",
        "Test Crypt",
        "Test Strip",
        "Test HQ",
        "Test Treasure",
    ];
    let mut rng = SplitMix64::new(17);
    let mut payable_positions = 0;
    for _ in 0..300 {
        let mut g = Game::new(Profile::EXACT);
        let mut names = Vec::new();
        let mut slots = Vec::new();
        for _ in 0..rng.below(6) {
            let name = SOURCES[rng.below(SOURCES.len() as u64) as usize];
            if g.s
                .library(g.me)
                .iter()
                .any(|&slot| g.t.def(g.s.object(slot).card).name == name)
            {
                slots.push(g.give(g.me, name, Zone::Battlefield));
                names.push(name);
            }
        }
        let mut pool = ManaPool::default();
        for color in [ManaColor::White, ManaColor::Blue, ManaColor::Colorless] {
            pool[color] = rng.below(2) as u8;
        }
        g.s.player_mut(g.me).mana = pool;
        let pool = pool.amounts();
        crate::layers::refresh(&mut g.s, &g.t, &mut Sink::default());
        let cost = ManaCost::new(
            rng.below(4) as u8,
            [rng.below(3) as u8, rng.below(3) as u8, 0, 0, 0, 0],
        );
        let expected = brute_force_pays(cost, pool, &names);
        let context = format!("{cost:?} from {pool:?} and {names:?}");
        let payable = mana::payable(&g.s, &g.t, g.me, cost, Bitboard::EMPTY).unwrap();
        assert_eq!(payable, expected, "{context}");
        for greedy in [false, true] {
            let plans = mana::plans(
                &g.s,
                &g.t,
                &mut Sink::default(),
                g.me,
                cost,
                Bitboard::EMPTY,
                greedy,
            )
            .unwrap();
            assert_eq!(!plans.is_empty(), payable, "{context}");
            let mut outcomes = Vec::new();
            for plan in &plans {
                let mut paid = g.s;
                mana::pay(
                    &mut paid,
                    &g.t,
                    &mut Sink::default(),
                    g.me,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                )
                .unwrap();
                // CR 601.2h: the cost is paid in full, and only from the mana the tapped and
                // sacrificed sources made and the pool held.
                let used = |slot: Slot| {
                    paid.object(slot).flags.has(ObjectFlags::TAPPED)
                        || paid.object(slot).zone == Zone::Graveyard
                };
                let made: u32 = slots
                    .iter()
                    .zip(&names)
                    .filter(|&(&slot, _)| used(slot))
                    .map(|(_, name)| u32::from(makes(name).1))
                    .sum();
                let sacrificed = slots
                    .iter()
                    .filter(|&&slot| paid.object(slot).zone == Zone::Graveyard)
                    .count();
                assert!(
                    !outcomes.contains(&(plan.left, sacrificed)),
                    "two plans leave {:?} and sacrifice {sacrificed}: {context}",
                    plan.left
                );
                outcomes.push((plan.left, sacrificed));
                let before: u32 = pool.iter().map(|&n| u32::from(n)).sum();
                let after: u32 = paid
                    .player(g.me)
                    .mana
                    .amounts()
                    .iter()
                    .map(|&n| u32::from(n))
                    .sum();
                assert_eq!(made + before - after, cost.total(0), "{context}");
                assert_eq!(paid.player(g.me).mana.amounts(), plan.left, "{context}");
            }
        }
        payable_positions += usize::from(payable);
    }
    assert!(payable_positions > 50, "only {payable_positions} payable");
}

/// The options of the open priority decision.
fn priority_options(g: &mut Game) -> Vec<Opt> {
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Priority);
    (0..d.len())
        .filter_map(|i| d.candidates.option(crate::Choice(i as u8)))
        .collect()
}

#[test]
fn a_treasure_adds_no_priority_option_of_its_own() {
    for payment in [Payment::KeepMostPayable, Payment::Greedy] {
        let mut g = Game::new(Profile {
            payment,
            ..Profile::EXACT
        });
        // A {1}{W}{U} spell, which a Plains and a Treasure can't pay.
        g.give(g.me, "Test Raveler", Zone::Hand);
        g.give(g.me, "Test Land", Zone::Battlefield);
        let without = priority_options(&mut g);
        g.give(g.me, "Test Treasure", Zone::Battlefield);
        assert_eq!(priority_options(&mut g), without, "{payment:?}");
    }
}

#[test]
fn a_spell_payable_only_with_a_treasure_sacrifices_it_as_it_is_cast() {
    for payment in [Payment::KeepMostPayable, Payment::Greedy] {
        let mut g = Game::new(Profile {
            payment,
            ..Profile::EXACT
        });
        let bear = g.give(g.me, "Test Bear", Zone::Hand);
        let land = g.give(g.me, "Test Land", Zone::Battlefield);
        let treasure = g.give(g.me, "Test Treasure", Zone::Battlefield);
        assert!(
            !priority_options(&mut g)
                .iter()
                .any(|o| matches!(o, Opt::Mana { .. })),
            "{payment:?}"
        );
        // {1}{W}: the Plains pays {W}, the Treasure {1}, with no question.
        g.pick(cast(bear));
        assert_eq!(g.s.object(bear).zone, Zone::Stack, "{payment:?}");
        assert!(tapped(&g, land), "{payment:?}");
        assert_eq!(g.s.object(treasure).zone, Zone::Graveyard, "{payment:?}");
        assert_eq!(g.s.player(g.me).mana, ManaPool::default(), "{payment:?}");
    }
}

#[test]
fn a_spell_payable_from_lands_keeps_the_treasure() {
    for payment in [Payment::KeepMostPayable, Payment::Greedy] {
        let mut g = Game::new(Profile {
            payment,
            ..Profile::EXACT
        });
        let bear = g.give(g.me, "Test Bear", Zone::Hand);
        let treasure = g.give(g.me, "Test Treasure", Zone::Battlefield);
        g.give(g.me, "Test Land", Zone::Battlefield);
        g.give(g.me, "Test Island", Zone::Battlefield);
        g.pick(cast(bear));
        assert_eq!(g.s.object(bear).zone, Zone::Stack, "{payment:?}");
        assert_eq!(g.s.object(treasure).zone, Zone::Battlefield, "{payment:?}");
        assert!(!tapped(&g, treasure), "{payment:?}");
    }
}

#[test]
fn sacrificing_a_treasure_is_offered_when_it_keeps_more_payable() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let strip = g.give(g.me, "Test Strip", Zone::Battlefield);
    let treasure = g.give(g.me, "Test Treasure", Zone::Battlefield);
    g.pick(cast(bear));
    // {1}{W}: the Strip pays {1} at the cost of its own ability, or the Treasure does at the
    // cost of itself. Keeping the Treasure comes first.
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::PaymentPlan);
    assert_eq!(d.len(), 2);
    g.pick(Opt::Plan(1));
    assert_eq!(g.s.object(treasure).zone, Zone::Graveyard);
    assert!(!tapped(&g, strip));
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
}

#[test]
fn the_greedy_payment_spends_the_strip_before_the_treasure() {
    let mut g = Game::new(Profile {
        payment: Payment::Greedy,
        ..Profile::EXACT
    });
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let strip = g.give(g.me, "Test Strip", Zone::Battlefield);
    let treasure = g.give(g.me, "Test Treasure", Zone::Battlefield);
    g.pick(cast(bear));
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
    assert!(tapped(&g, strip));
    assert_eq!(g.s.object(treasure).zone, Zone::Battlefield);
}

#[test]
fn manual_payment_offers_the_treasure_as_a_source() {
    let mut g = Game::new(Profile {
        payment: Payment::Manual,
        ..Profile::EXACT
    });
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let treasure = g.give(g.me, "Test Treasure", Zone::Battlefield);
    // CR 605.3a: a standalone activation, at priority, under manual payment.
    assert!(priority_options(&mut g).contains(&Opt::Mana {
        slot: treasure,
        color: ManaColor::Blue,
    }));
    g.pick(cast(bear));
    g.pick(Opt::Mana {
        slot: treasure,
        color: ManaColor::Blue,
    });
    assert_eq!(g.s.object(treasure).zone, Zone::Graveyard);
    assert_eq!(g.s.object(bear).zone, Zone::Stack);
}
