//! The mana solver (architecture §10, decision D7): payability, plans, and which payments are
//! decisions.

use super::LAND;
use super::cast::{BEAR, Game, ISLAND, LIES, TREASURE, cast};
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Cost, CostPart, Face, ManaColor, ManaCost, Types};
use crate::ids::Slot;
use crate::mana::{self, Units};
use crate::replacement::event::Sink;
use crate::rng::SplitMix64;
use crate::state::{ManaPool, TurnStep, Zone};
use crate::{CardTable, DecisionKind, Opt, Payment, Profile};
use beryl_card::effect::build::{draw, you};

fn tapped(g: &Game, slot: Slot) -> bool {
    g.s.object(slot).is_tapped()
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
fn offering_payments_with_a_full_pool_does_not_wrap_remaining_mana() {
    let mut g = Game::new(Profile::EXACT);
    g.give(g.me, "Test Crypt", Zone::Battlefield);
    let color = ManaColor::Colorless;
    let mut pool = ManaPool::default();
    pool[color] = u8::MAX;
    g.s.board().set_mana(g.me, pool);
    crate::continuous::layers::refresh(&mut g.s, &g.t, &mut Sink::default()).unwrap();

    let plans = mana::plans(
        &g.s,
        &g.t,
        &mut Sink::default(),
        g.me,
        ManaCost::generic(1),
        Bitboard::EMPTY,
        false,
        mana::Paying::NOTHING,
    )
    .unwrap();
    assert!(plans.iter().any(|plan| plan.left[color.index()] == 254));
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
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
    (tapped(&g, strip), tapped(&g, dual))
}

#[test]
fn the_exact_payment_keeps_the_most_costs_payable() {
    // Tapping the Strip would cost its own ability; the dual has no other use.
    assert_eq!(strip_or_dual(Payment::KeepMostPayable), (false, true));
}

/// Casts a {1}{U} flash creature on the opponent's end step with a Plains and three
/// Islands, holding a {U}{U} sorcery and a {1}{W} instant, `g.me` tapping their own mana if
/// `human`, and says whether the Plains stayed untapped.
fn flash_keeps_the_plains(human: bool) -> bool {
    let mut g = Game::new(Profile::EXACT);
    if human {
        g.s.set_profile(tapping(&g));
    }
    let flasher = g.give(g.me, "Test Flasher", Zone::Hand);
    g.give(g.me, "Test Sage", Zone::Hand);
    g.give(g.me, "Test Removal", Zone::Hand);
    let plains = g.give(g.me, "Test Land", Zone::Battlefield);
    for _ in 0..3 {
        g.give(g.me, "Test Island", Zone::Battlefield);
    }
    g.at(g.them, TurnStep::End);
    g.s.board().set_priority(g.me);
    g.pick(cast(flasher));
    assert_eq!(g.s.object(flasher).zone(), Zone::Stack);
    !tapped(&g, plains)
}

#[test]
fn a_human_payment_on_the_opponents_turn_keeps_only_instants_payable() {
    // Two Islands left would keep the {U}{U} sorcery payable, which can't be cast before the
    // lands untap; the Plains and an Island keep the {1}{W} instant.
    assert!(flash_keeps_the_plains(true));
}

#[test]
fn only_the_human_payment_looks_ahead() {
    // Search's payments keep the costs in hand payable whatever their timing, as cheaply as
    // a rollout needs.
    assert!(!flash_keeps_the_plains(false));
}

#[test]
fn a_human_payment_keeps_two_copies_of_a_card_payable_together() {
    // A Plains and the dual left pay one of the {U} instants; an Island and the dual both.
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(tapping(&g));
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Lies", Zone::Hand);
    g.give(g.me, "Test Lies", Zone::Hand);
    let island = g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let dual = g.give(g.me, "Test Dual", Zone::Battlefield);
    g.pick(cast(bear));
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
    assert!(!tapped(&g, island) && !tapped(&g, dual));
}

/// Casts a {1}{U} creature in `g.me`'s main phase with a Plains and two Islands, holding an
/// Island to play, a {1}{W} creature and a {U} sorcery, `g.me` tapping their own mana, and
/// says whether the Plains stayed untapped.
fn main_phase_keeps_the_plains(lands_played: u8) -> bool {
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(tapping(&g));
    let diver = g.give(g.me, "Test Diver", Zone::Hand);
    g.give(g.me, "Test Island", Zone::Hand);
    g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Ponder", Zone::Hand);
    let plains = g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    g.s.board().set_lands_played(g.me, lands_played);
    g.pick(cast(diver));
    assert_eq!(g.s.object(diver).zone(), Zone::Stack);
    !tapped(&g, plains)
}

#[test]
fn a_human_payment_counts_the_land_still_to_be_played() {
    // The Plains and the Island to play cast the {1}{W} creature; two Islands only the {U}.
    assert!(main_phase_keeps_the_plains(0));
    // With the land played, an Island left keeps the sorcery and the Plains nothing.
    assert!(!main_phase_keeps_the_plains(1));
}

/// Casts a {1}{W} creature with three Plains and an Island, holding a {U} instant, with a
/// creature on the battlefield whose ability costs {W}{W} and {T}, and says whether the
/// Island stayed untapped. Two Plains left would pay that ability, and the Plains and the
/// Island left would pay the instant.
fn a_tap_ability_keeps_the_plains(sick: bool) -> bool {
    static PRIEST: CardDef = beryl_card::definition::card(
        "Test Priest",
        &[Face {
            cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
            types: Types::CREATURE,
            power: Some(1),
            toughness: Some(1),
            abilities: &[Ability::activated(Cost::of(
                ManaCost::new(0, [2, 0, 0, 0, 0, 0]),
                &[CostPart::TAP],
            ))
            .effect(&draw(you(), 1))],
            ..Face::EMPTY
        }],
    );
    static CARDS: CardTable = CardTable::new(&[&PRIEST, &LAND, &ISLAND, &BEAR, &LIES]);
    let deck = &[
        (8, LAND.name),
        (2, ISLAND.name),
        (1, BEAR.name),
        (1, LIES.name),
        (1, PRIEST.name),
    ];
    let mut g = Game::with(&CARDS, deck, Profile::EXACT);
    let bear = g.give(g.me, BEAR.name, Zone::Hand);
    g.give(g.me, LIES.name, Zone::Hand);
    let priest = g.give(g.me, PRIEST.name, Zone::Battlefield);
    g.s.board().set_sick(priest, sick);
    for _ in 0..3 {
        g.give(g.me, LAND.name, Zone::Battlefield);
    }
    let island = g.give(g.me, ISLAND.name, Zone::Battlefield);
    g.pick(cast(bear));
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
    !tapped(&g, island)
}

#[test]
fn a_payment_keeps_mana_only_for_a_tap_ability_that_can_be_activated() {
    // The two Plains left pay the creature's {W}{W}, which is more than the instant's {U}.
    assert!(!a_tap_ability_keeps_the_plains(false));
    // CR 302.6: a creature that hasn't been under its controller's control since their turn
    // began can't pay {T}, so the payment keeps the instant payable instead.
    assert!(a_tap_ability_keeps_the_plains(true));
}

#[test]
fn a_human_payment_taps_lands_before_a_mana_creature() {
    // CR 508.1a: an untapped creature may still attack, so even the dual taps first.
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(tapping(&g));
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let elf = g.give(g.me, "Test Elf", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Dual", Zone::Battlefield);
    g.pick(cast(bear));
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
    assert!(!tapped(&g, elf));
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
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
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
            matches!(d.candidates.option(crate::Choice(i as u16)), Some(Opt::Mana { slot, .. }) if slot == dual)
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
        assert_eq!(g.s.object(overlord).zone(), Zone::Stack, "{payment:?}");
        assert!(tapped(&g, crypt), "{payment:?}");
        assert_eq!(
            lands.iter().filter(|&&l| tapped(&g, l)).count(),
            2,
            "{payment:?}"
        );
        assert_eq!(g.s.player(g.me).mana(), ManaPool::default(), "{payment:?}");
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

/// Pays `cost` by `plan` on a copy of `g`, whose sources `slots` are the cards `names`, and
/// checks the payment: the floating mana and how many sources it sacrificed afterwards.
fn pay_and_check(
    g: &Game,
    slots: &[Slot],
    names: &[&str],
    cost: ManaCost,
    plan: &mana::Plan,
    context: &str,
) -> ([u8; 6], usize) {
    let pool = g.s.player(g.me).mana().amounts();
    let mut paid = g.s;
    mana::pay(
        &mut paid,
        &g.t,
        &mut Sink::default(),
        g.me,
        cost,
        Bitboard::EMPTY,
        plan,
        crate::mana::Paying::NOTHING,
    )
    .unwrap();
    // CR 601.2h: the cost is paid in full, and only from the mana the tapped and sacrificed
    // sources made and the pool held.
    let sacrificed = |slot: Slot| paid.object(slot).zone() == Zone::Graveyard;
    let used = |slot: Slot| paid.object(slot).is_tapped() || sacrificed(slot);
    let made: u32 = slots
        .iter()
        .zip(names)
        .filter(|&(&slot, _)| used(slot))
        .map(|(_, name)| u32::from(makes(name).1))
        .sum();
    let total = |amounts: [u8; 6]| amounts.iter().map(|&n| u32::from(n)).sum::<u32>();
    let after = paid.player(g.me).mana().amounts();
    assert_eq!(
        made + total(pool) - total(after),
        cost.total(0),
        "{context}"
    );
    assert_eq!(after, plan.left, "{context}");
    (
        after,
        slots.iter().filter(|&&slot| sacrificed(slot)).count(),
    )
}

/// Calls `each` with 300 random positions of the test sources: a game whose `g.me` controls
/// some of them, under the names given, with up to one {W}, {U} and {C} floating, and a cost.
fn random_positions(mut each: impl FnMut(&Game, &[Slot], &[&str], [u8; 6], ManaCost)) {
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
    for _ in 0..300 {
        let mut g = Game::new(Profile::EXACT);
        let mut names = Vec::new();
        let mut slots = Vec::new();
        for _ in 0..rng.below(6) {
            let name = SOURCES[rng.below(SOURCES.len() as u64) as usize];
            if g.s
                .library(g.me)
                .iter()
                .any(|&slot| g.s.definition_name(&g.t, slot) == name)
            {
                slots.push(g.give(g.me, name, Zone::Battlefield));
                names.push(name);
            }
        }
        let mut pool = ManaPool::default();
        for color in [ManaColor::White, ManaColor::Blue, ManaColor::Colorless] {
            pool[color] = rng.below(2) as u8;
        }
        g.s.board().set_mana(g.me, pool);
        crate::continuous::layers::refresh(&mut g.s, &g.t, &mut Sink::default()).unwrap();
        let cost = ManaCost::new(
            rng.below(4) as u8,
            [rng.below(3) as u8, rng.below(3) as u8, 0, 0, 0, 0],
        );
        each(&g, &slots, &names, pool.amounts(), cost);
    }
}

#[test]
fn the_solver_pays_exactly_the_costs_brute_force_can_pay() {
    let mut payable_positions = 0;
    random_positions(|g, slots, names, pool, cost| {
        let expected = brute_force_pays(cost, pool, names);
        let context = format!("{cost:?} from {pool:?} and {names:?}");
        let payable = crate::mana::payable(
            &g.s,
            &g.t,
            g.me,
            cost,
            Bitboard::EMPTY,
            crate::mana::Paying::NOTHING,
        )
        .unwrap();
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
                crate::mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(!plans.is_empty(), payable, "{context}");
            let mut outcomes = Vec::new();
            for plan in &plans {
                let outcome = pay_and_check(g, slots, names, cost, plan, &context);
                assert!(
                    !outcomes.contains(&outcome),
                    "two plans leave {:?} and sacrifice {}: {context}",
                    outcome.0,
                    outcome.1
                );
                outcomes.push(outcome);
            }
        }
        payable_positions += usize::from(payable);
    });
    assert!(payable_positions > 50, "only {payable_positions} payable");
}

/// With no priced mana ability in play, every order of the mana abilities pays what one tap of
/// each source pays: the reference of sequences answers as the reference of units does, at
/// every position of the brute-force test above.
#[test]
fn the_sequence_reference_agrees_with_the_unit_reference() {
    let mut payable_positions = 0;
    random_positions(|g, _, names, pool, cost| {
        let context = format!("{cost:?} from {pool:?} and {names:?}");
        let units = crate::reference::reference_payable(&g.s, &g.t, g.me, cost);
        let sequences = crate::reference::reference_sequences(&g.s, &g.t, g.me, cost);
        assert!(units.is_some(), "{context}");
        assert_eq!(sequences, units, "{context}");
        payable_positions += usize::from(units == Some(true));
    });
    assert!(payable_positions > 50, "only {payable_positions} payable");
}

/// The options of the open priority decision.
fn priority_options(g: &mut Game) -> Vec<Opt> {
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::Priority);
    (0..d.len())
        .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
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
fn mana_payment_leaves_an_artifact_for_the_sacrifice_cost() {
    static SPELL: CardDef = beryl_card::definition::card(
        "Test Artifact Sacrifice",
        &[Face {
            cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
            types: Types::INSTANT,
            additional_cost: &[CostPart::sacrifice(beryl_card::Filter::Types(
                Types::ARTIFACT,
            ))],
            abilities: &[Ability::spell()],
            ..Face::EMPTY
        }],
    );
    static CARDS: CardTable = CardTable::new(&[&SPELL, &LAND, &TREASURE]);
    let deck = &[(8, LAND.name), (2, TREASURE.name), (1, SPELL.name)];
    for payment in [Payment::KeepMostPayable, Payment::Greedy, Payment::Manual] {
        let mut game = Game::with(
            &CARDS,
            deck,
            Profile {
                payment,
                ..Profile::EXACT
            },
        );
        let spell = game.give(game.me, SPELL.name, Zone::Hand);
        let land = game.give(game.me, LAND.name, Zone::Hand);
        let first = game.give(game.me, TREASURE.name, Zone::Battlefield);
        let second = game.give(game.me, TREASURE.name, Zone::Battlefield);
        assert!(
            !priority_options(&mut game).contains(&cast(spell)),
            "{payment:?}"
        );

        game.pick(Opt::PlayLand(land));
        game.pick(cast(spell));
        if payment == Payment::Manual {
            game.pick(Opt::Mana {
                slot: first.min(second),
                color: ManaColor::White,
            });
        }

        assert_eq!(game.s.object(spell).zone(), Zone::Stack, "{payment:?}");
        assert_eq!(game.s.object(first).zone(), Zone::Graveyard, "{payment:?}");
        assert_eq!(game.s.object(second).zone(), Zone::Graveyard, "{payment:?}");
        assert!(tapped(&game, land), "{payment:?}");
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
        assert_eq!(g.s.object(bear).zone(), Zone::Stack, "{payment:?}");
        assert!(tapped(&g, land), "{payment:?}");
        assert_eq!(g.s.object(treasure).zone(), Zone::Graveyard, "{payment:?}");
        assert_eq!(g.s.player(g.me).mana(), ManaPool::default(), "{payment:?}");
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
        assert_eq!(g.s.object(bear).zone(), Zone::Stack, "{payment:?}");
        assert_eq!(
            g.s.object(treasure).zone(),
            Zone::Battlefield,
            "{payment:?}"
        );
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
    assert_eq!(g.s.object(treasure).zone(), Zone::Graveyard);
    assert!(!tapped(&g, strip));
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
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
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
    assert!(tapped(&g, strip));
    assert_eq!(g.s.object(treasure).zone(), Zone::Battlefield);
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
    assert_eq!(g.s.object(treasure).zone(), Zone::Graveyard);
    assert_eq!(g.s.object(bear).zone(), Zone::Stack);
}

/// The exact profile, with `g.me` tapping their mana sources at priority as a human may.
fn tapping(g: &Game) -> Profile {
    Profile {
        taps_mana: Some(g.me),
        ..Profile::EXACT
    }
}

#[test]
fn the_named_player_taps_a_source_at_priority_and_pays_with_its_mana() {
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(tapping(&g));
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    let land = g.give(g.me, "Test Land", Zone::Battlefield);
    let dual = g.give(g.me, "Test Dual", Zone::Battlefield);
    let island = g.give(g.me, "Test Island", Zone::Battlefield);
    // CR 605.3a: each source, for each kind of mana it makes.
    let options = priority_options(&mut g);
    for (slot, color) in [
        (land, ManaColor::White),
        (dual, ManaColor::White),
        (dual, ManaColor::Blue),
        (island, ManaColor::Blue),
    ] {
        assert!(options.contains(&Opt::Mana { slot, color }), "{options:?}");
    }
    g.pick(Opt::Mana {
        slot: dual,
        color: ManaColor::Blue,
    });
    assert!(tapped(&g, dual));
    assert_eq!(g.s.player(g.me).mana().amounts(), [0, 1, 0, 0, 0, 0]);
    // CR 117.3c: they keep priority, and may still cast with the mana.
    assert!(g.offers_cast(bear));
    // The floating {U} pays the {1}, not the Island leaving it floating (§10).
    g.pick(cast(bear));
    assert_eq!(
        g.s.object(bear).zone(),
        Zone::Stack,
        "paid without a question"
    );
    assert!(tapped(&g, land));
    assert!(!tapped(&g, island));
    assert_eq!(g.s.player(g.me).mana(), ManaPool::default());
}

#[test]
fn floating_mana_no_source_left_could_make_is_still_a_choice() {
    let mut g = Game::new(Profile::EXACT);
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    let mut pool = ManaPool::default();
    pool[ManaColor::Blue] = 1;
    g.s.board().set_mana(g.me, pool);
    g.pick(cast(bear));
    // {U} and a Plains, or two Plains keeping the {U}: no Plains makes {U} again.
    let d = g.decide();
    assert_eq!(d.kind, DecisionKind::PaymentPlan);
    assert_eq!(d.len(), 2);
    // The engine's default, the first plan, spends the floating mana.
    g.choose(crate::Choice(0));
    assert_eq!(g.s.player(g.me).mana(), ManaPool::default());
}

#[test]
fn only_the_named_player_is_offered_mana_at_priority() {
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(Profile {
        taps_mana: Some(g.them),
        ..Profile::EXACT
    });
    let bear = g.give(g.me, "Test Bear", Zone::Hand);
    g.give(g.me, "Test Land", Zone::Battlefield);
    g.give(g.me, "Test Island", Zone::Battlefield);
    let options = priority_options(&mut g);
    assert!(options.contains(&cast(bear)));
    assert!(!options.iter().any(|o| matches!(o, Opt::Mana { .. })));
}

#[test]
fn a_priority_with_only_mana_to_make_passes_silently() {
    let mut g = Game::new(Profile::EXACT);
    g.s.set_profile(tapping(&g));
    let land = g.give(g.me, "Test Land", Zone::Battlefield);
    let d = g.decide();
    // Floating mana nothing can spend changes nothing (CR 106.4), so the engine passes.
    let offers_mana = (0..d.len()).any(|i| {
        matches!(
            d.candidates.option(crate::Choice(i as u16)),
            Some(Opt::Mana { .. })
        )
    });
    assert!(!(d.player == g.me && offers_mana), "{d:?}");
    assert!(!tapped(&g, land));
}

/// "{T}: Add {G}. Activate only as an instant."
static INSTANT_ELF: CardDef = CardDef {
    name: "Test Instant Mana Elf",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::tap_for_mana(crate::card::ManaSet {
            colors: 1 << 4,
            amount: 1,
        })
        .as_an_instant()],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature's {T} mana ability activated only as an instant is offered at priority only once
/// the creature has been under its controller's control since their turn began (see CR 302.6).
#[test]
fn a_summoning_sick_creature_offers_no_instant_mana_ability() {
    static CARDS: CardTable = CardTable::new(&[&INSTANT_ELF, &LAND]);
    let deck: &[(u8, &str)] = &[(4, "Test Instant Mana Elf"), (36, "Test Land")];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 5, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let elf = super::chosen::to_hand(&t, &mut s, p, "Test Instant Mana Elf");
    s.board().place(elf, Zone::Battlefield).unwrap();
    s.board().set_sick(elf, true);
    let refresh = |s: &mut crate::State| {
        crate::continuous::layers::refresh(s, &t, &mut Sink::off()).unwrap();
    };
    refresh(&mut s);
    let offered = |s: &crate::State| {
        let mut out = crate::fixed::Fixed::default();
        mana::instant_options(s, &t, p, &mut out).unwrap();
        out.iter()
            .any(|o| matches!(o, Opt::Activate { slot, .. } if *slot == elf))
    };
    assert!(!offered(&s), "summoning sick");
    s.board().set_sick(elf, false);
    refresh(&mut s);
    assert!(offered(&s));
}

static TEST_SIGNET: CardDef = CardDef {
    name: "Test Signet",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana_of(
            Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
            &[
                beryl_card::Term::one(ManaColor::White),
                beryl_card::Term::one(ManaColor::Blue),
            ],
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_FILTER_GATE: CardDef = CardDef {
    name: "Test Filter Gate",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::tap_for_mana(beryl_card::ManaSet::one(ManaColor::Colorless)),
            Ability::mana_of(
                Cost::of(beryl_card::mana("{W/U}"), &[CostPart::TAP]),
                &[beryl_card::Term::each_of(0b11, 2)],
            ),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_KAROO: CardDef = CardDef {
    name: "Test Karoo",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(
            Cost::TAP,
            &[
                beryl_card::Term::one(ManaColor::White),
                beryl_card::Term::one(ManaColor::Blue),
            ],
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_SWAMP: CardDef = CardDef {
    name: "Test Swamp",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        subtypes: beryl_card::SubtypeWords::new(beryl_card::Subtypes::of(&[
            beryl_card::Subtype::Swamp,
        ])),
        abilities: &[Ability::tap_for_mana(beryl_card::ManaSet::one(
            ManaColor::Black,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_WASTES: CardDef = CardDef {
    name: "Test Wastes",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::tap_for_mana(beryl_card::ManaSet::one(
            ManaColor::Colorless,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static SWAMPS_YOU_CONTROL: beryl_card::Objects = beryl_card::Objects::InZoneOf {
    zone: Zone::Battlefield,
    players: &beryl_card::Players::You,
    filter: beryl_card::Filter::Subtype(beryl_card::Subtype::Swamp),
};

static TEST_COFFERS: CardDef = CardDef {
    name: "Test Coffers",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(
            Cost::of(ManaCost::generic(2), &[CostPart::TAP]),
            &[beryl_card::Term::counted(
                ManaColor::Black,
                beryl_card::Amount::Count(&SWAMPS_YOU_CONTROL),
            )],
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_SWAMP_COUNT_LAND: CardDef = CardDef {
    name: "Test Swamp Count Land",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(
            Cost::TAP,
            &[beryl_card::Term::counted(
                ManaColor::Black,
                beryl_card::Amount::Count(&SWAMPS_YOU_CONTROL),
            )],
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// `rich::pays`, `mana::payable`, `mana::plans` (under both `KeepMostPayable` and `Greedy`) and
/// `Payment::Manual` agree with `reference_sequences` across 200 seeded boards of up to four
/// priced sources and six lands.
#[test]
#[allow(clippy::too_many_lines)]
fn rich_pays_and_plans_agree_with_reference_sequences_on_generated_boards() {
    static CARDS: CardTable = CardTable::new(&[
        &LAND,
        &ISLAND,
        &super::DUAL,
        &TEST_SWAMP,
        &TEST_WASTES,
        &TEST_SIGNET,
        &TEST_FILTER_GATE,
        &TEST_KAROO,
    ]);
    let deck: &[(u8, &str)] = &[
        (6, "Test Land"),
        (6, "Test Island"),
        (6, "Test Dual"),
        (6, "Test Swamp"),
        (6, "Test Wastes"),
        (4, "Test Signet"),
        (4, "Test Filter Gate"),
        (4, "Test Karoo"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let plain_cards = [
        "Test Land",
        "Test Island",
        "Test Dual",
        "Test Swamp",
        "Test Wastes",
    ];
    let priced_cards = ["Test Signet", "Test Filter Gate", "Test Karoo"];
    let mut rng = SplitMix64::new(0x5160_E707);
    let mut payable_count = 0usize;
    let mut manual_steps = 0usize;
    for board_ix in 0..200 {
        let mut s = crate::State::new(&t, board_ix as u64 + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let n_priced = rng.below(4) as usize + 1;
        let n_plain = rng.below(7) as usize;
        let mut placed = Vec::new();
        for _ in 0..n_priced {
            let name = priced_cards[rng.below(priced_cards.len() as u64) as usize];
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == name)
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                placed.push(name);
            }
        }
        for _ in 0..n_plain {
            let name = plain_cards[rng.below(plain_cards.len() as u64) as usize];
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == name)
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                placed.push(name);
            }
        }
        let mut pool = ManaPool::default();
        for color in [ManaColor::White, ManaColor::Blue, ManaColor::Colorless] {
            pool[color] = rng.below(2) as u8;
        }
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        let cost = ManaCost::new(
            rng.below(4) as u8,
            [rng.below(3) as u8, rng.below(3) as u8, 0, 0, 0, 0],
        );
        let Some(expected) = crate::reference::reference_sequences(&s, &t, p, cost) else {
            continue;
        };
        let ctx = format!(
            "board {board_ix}: cost {cost:?}, pool {:?}, placed {placed:?}",
            pool.amounts()
        );
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        assert_eq!(
            funds.pays_for(&s, &t, cost, Bitboard::EMPTY, mana::Paying::NOTHING),
            expected,
            "Funds::pays_for disagreed: {ctx}"
        );
        assert_eq!(
            mana::payable(&s, &t, p, cost, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap(),
            expected,
            "mana::payable disagreed: {ctx}"
        );
        for greedy in [false, true] {
            let plans = mana::plans(
                &s,
                &t,
                &mut Sink::off(),
                p,
                cost,
                Bitboard::EMPTY,
                greedy,
                mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(
                !plans.is_empty(),
                expected,
                "mana::plans(greedy={greedy}) disagreed: {ctx}"
            );
            if greedy {
                assert!(plans.len() <= 1, "greedy returned >1 plan: {ctx}");
            }
            let mut seen_outcomes = Vec::new();
            for plan in &plans {
                let mut paid_state = s;
                let mut sink = Sink::off();
                mana::pay(
                    &mut paid_state,
                    &t,
                    &mut sink,
                    p,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert_eq!(
                    paid_state.player(p).mana().amounts(),
                    plan.left,
                    "plan.left mismatch: {ctx}"
                );
                assert!(
                    !seen_outcomes.contains(&plan.left),
                    "duplicate outcome {:?}: {ctx}",
                    plan.left
                );
                seen_outcomes.push(plan.left);
            }
        }
        if expected && !cost.is_zero() {
            payable_count += 1;
            // Under Payment::Manual, every sequence of offered options ends with the cost paid.
            for trial in 0..3 {
                let mut ms = s;
                let mut rem = cost;
                let mut guard = 0;
                while !rem.is_zero() {
                    let opts = mana::manual_options(
                        &ms,
                        &t,
                        p,
                        rem,
                        Bitboard::EMPTY,
                        mana::Paying::NOTHING,
                    )
                    .unwrap();
                    assert!(
                        !opts.is_empty(),
                        "manual_options empty with remaining {rem:?} (trial {trial}): {ctx}"
                    );
                    let pick_ix = match trial {
                        0 => 0,
                        1 => opts.len() - 1,
                        _ => rng.below(opts.len() as u64) as usize,
                    };
                    let opt = opts.as_slice()[pick_ix];
                    rem = mana::manual_pay(
                        &mut ms,
                        &t,
                        &mut Sink::off(),
                        p,
                        rem,
                        opt,
                        mana::Paying::NOTHING,
                    )
                    .unwrap()
                    .0;
                    manual_steps += 1;
                    guard += 1;
                    assert!(guard < 40, "manual payment did not terminate: {ctx}");
                }
            }
        }
    }
    assert!(payable_count > 50, "only {payable_count} payable boards");
    assert!(manual_steps > 100, "only {manual_steps} manual steps");
}

/// A mana ability whose term counts permanents on the battlefield ("for each Swamp you control")
/// evaluates the count for the board as it stands, both unpriced and priced.
#[test]
#[allow(clippy::too_many_lines)]
fn counted_mana_ability_reads_the_board() {
    static CARDS: CardTable = CardTable::new(&[
        &TEST_SWAMP,
        &TEST_WASTES,
        &TEST_COFFERS,
        &TEST_SWAMP_COUNT_LAND,
    ]);
    let deck: &[(u8, &str)] = &[
        (20, "Test Swamp"),
        (12, "Test Wastes"),
        (4, "Test Coffers"),
        (4, "Test Swamp Count Land"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 42, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let place = |s: &mut crate::State, name: &str| {
        let &slot = s
            .library(p)
            .iter()
            .find(|&&sl| s.definition_name(&t, sl) == name)
            .unwrap();
        s.board().place(slot, Zone::Battlefield).unwrap();
    };
    place(&mut s, "Test Swamp Count Land");
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    let b1 = ManaCost::new(0, [0, 0, 1, 0, 0, 0]);
    let b3 = ManaCost::new(0, [0, 0, 3, 0, 0, 0]);
    let b6 = ManaCost::new(0, [0, 0, 6, 0, 0, 0]);
    // 0 Swamps -> produces 0 mana.
    assert!(!mana::payable(&s, &t, p, b1, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap());
    assert_eq!(
        crate::reference::reference_sequences(&s, &t, p, b1),
        Some(false)
    );
    // Add 3 Swamps -> Test Swamp Count Land alone makes {B}{B}{B}, plus 3 Swamps = 6 {B}.
    for _ in 0..3 {
        place(&mut s, "Test Swamp");
    }
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    assert!(mana::payable(&s, &t, p, b6, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap());
    assert_eq!(
        crate::reference::reference_sequences(&s, &t, p, b6),
        Some(true)
    );
    let plans = mana::plans(
        &s,
        &t,
        &mut Sink::off(),
        p,
        b6,
        Bitboard::EMPTY,
        false,
        mana::Paying::NOTHING,
    )
    .unwrap();
    assert!(!plans.is_empty());
    let mut paid = s;
    mana::pay(
        &mut paid,
        &t,
        &mut Sink::off(),
        p,
        b6,
        Bitboard::EMPTY,
        &plans.as_slice()[0],
        mana::Paying::NOTHING,
    )
    .unwrap();
    assert_eq!(paid.player(p).mana().amounts(), [0; 6]);
    // Add Test Coffers ({2}, {T}: Add {B} for each Swamp) + 2 Wastes -> can pay 9 {B} total
    // (3 Swamps + 3 from Swamp Count Land + 3 from Coffers paid by 2 Wastes).
    place(&mut s, "Test Coffers");
    place(&mut s, "Test Wastes");
    place(&mut s, "Test Wastes");
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    let b9 = ManaCost::new(0, [0, 0, 9, 0, 0, 0]);
    let b10 = ManaCost::new(0, [0, 0, 10, 0, 0, 0]);
    assert!(mana::payable(&s, &t, p, b9, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap());
    assert!(!mana::payable(&s, &t, p, b10, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap());
    assert_eq!(
        crate::reference::reference_sequences(&s, &t, p, b9),
        Some(true)
    );
    assert_eq!(
        crate::reference::reference_sequences(&s, &t, p, b10),
        Some(false)
    );
    // And manual payment pays {B}{B}{B}{B}{B}{B}{B}{B}{B} cleanly.
    let mut ms = s;
    let mut rem = b9;
    while !rem.is_zero() {
        let opts =
            mana::manual_options(&ms, &t, p, rem, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap();
        assert!(!opts.is_empty());
        rem = mana::manual_pay(
            &mut ms,
            &t,
            &mut Sink::off(),
            p,
            rem,
            opts.as_slice()[0],
            mana::Paying::NOTHING,
        )
        .unwrap()
        .0;
    }
    let _ = b3;
}

static TEST_HIGH_TIDE_CHANGE: beryl_card::ManaChange = beryl_card::ManaChange::Adds {
    tapped: beryl_card::Tapped::A(beryl_card::Filter::Subtype(beryl_card::Subtype::Island)),
    added: beryl_card::Added::Of(beryl_card::ManaSet::one(ManaColor::Blue)),
};

static TEST_HIGH_TIDE_EFFECT: beryl_card::Effect =
    beryl_card::effect::build::change_mana(&TEST_HIGH_TIDE_CHANGE, beryl_card::Duration::EndOfTurn);

static TEST_HIGH_TIDE: CardDef = CardDef {
    name: "Test High Tide",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::INSTANT,
        cost: beryl_card::mana("{U}"),
        abilities: &[Ability::spell().effect(&TEST_HIGH_TIDE_EFFECT)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_TUNDRA: CardDef = CardDef {
    name: "Test Tundra",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        subtypes: beryl_card::SubtypeWords::new(beryl_card::Subtypes::of(&[
            beryl_card::Subtype::Plains,
            beryl_card::Subtype::Island,
        ])),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// On 200 seeded boards with one or two live mana-changing effects, the solver's payability,
/// plans (both `KeepMostPayable` and `Greedy`), and `Payment::Manual` agree with `reference_sequences`.
#[test]
#[allow(clippy::too_many_lines)]
fn reference_agrees_with_mana_changes_on_200_seeded_boards() {
    static CARDS: CardTable = CardTable::new(&[
        &LAND,
        &ISLAND,
        &TEST_TUNDRA,
        &TEST_SWAMP,
        &TEST_SIGNET,
        &TEST_HIGH_TIDE,
    ]);
    let deck: &[(u8, &str)] = &[
        (10, "Test Land"),
        (10, "Test Island"),
        (10, "Test Tundra"),
        (10, "Test Swamp"),
        (4, "Test Signet"),
        (4, "Test High Tide"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let lands = [
        "Test Land",
        "Test Island",
        "Test Tundra",
        "Test Swamp",
        "Test Signet",
    ];
    let mut rng = SplitMix64::new(0x4168_71DE);
    let mut payable_count = 0usize;
    let mut manual_steps = 0usize;
    for board_ix in 0..200 {
        let mut s = crate::State::new(&t, board_ix as u64 + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let n_lands = rng.below(6) as usize + 1;
        let mut placed = Vec::new();
        for _ in 0..n_lands {
            let name = lands[rng.below(lands.len() as u64) as usize];
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == name)
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                placed.push(name);
            }
        }
        // Start 1 or 2 live High Tide effects (with occasional captured Island -> Swamp text change).
        let n_tides = rng.below(2) as usize + 1;
        let tide_card = s
            .library(p)
            .iter()
            .find(|&&sl| s.definition_name(&t, sl) == "Test High Tide")
            .map(|&sl| s.definition(sl).record(&t).0)
            .unwrap();
        let hacked = rng.below(5) == 0;
        let words = if hacked {
            beryl_card::Words::NONE.changing(beryl_card::TextChange::LandType {
                from: beryl_card::words::BasicLandWord::of_subtype(beryl_card::Subtype::Island)
                    .unwrap(),
                to: beryl_card::words::BasicLandWord::of_subtype(beryl_card::Subtype::Swamp)
                    .unwrap(),
            })
        } else {
            beryl_card::Words::NONE
        };
        for _ in 0..n_tides {
            s.start_effect(
                tide_card,
                crate::state::Continuous::Mana(crate::state::ManaEffect {
                    face: beryl_card::FaceIx::FRONT,
                    ability: beryl_card::AbilityIx::new(0),
                    part: 0,
                    controller: p,
                    colors: words.color_changes(),
                    lands: words.land_changes(),
                }),
                crate::state::Duration::EndOfTurn,
            )
            .unwrap();
        }
        let mut pool = ManaPool::default();
        for color in [ManaColor::White, ManaColor::Blue, ManaColor::Black] {
            pool[color] = rng.below(2) as u8;
        }
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        let cost = ManaCost::new(
            rng.below(5) as u8,
            [
                rng.below(3) as u8,
                rng.below(4) as u8,
                rng.below(2) as u8,
                0,
                0,
                0,
            ],
        );
        let Some(expected) = crate::reference::reference_sequences(&s, &t, p, cost) else {
            continue;
        };
        let ctx = format!(
            "board {board_ix}: tides={n_tides} hacked={hacked} cost={cost:?} pool={:?} placed={placed:?}",
            pool.amounts()
        );
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        assert_eq!(
            funds.pays_for(&s, &t, cost, Bitboard::EMPTY, mana::Paying::NOTHING),
            expected,
            "Funds::pays_for disagreed: {ctx}"
        );
        assert_eq!(
            mana::payable(&s, &t, p, cost, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap(),
            expected,
            "mana::payable disagreed: {ctx}"
        );
        for greedy in [false, true] {
            let plans = mana::plans(
                &s,
                &t,
                &mut Sink::off(),
                p,
                cost,
                Bitboard::EMPTY,
                greedy,
                mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(
                !plans.is_empty(),
                expected,
                "mana::plans(greedy={greedy}) disagreed: {ctx}"
            );
            if greedy {
                assert!(plans.len() <= 1, "greedy returned >1 plan: {ctx}");
            }
            for plan in &plans {
                let mut paid = s;
                mana::pay(
                    &mut paid,
                    &t,
                    &mut Sink::off(),
                    p,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert_eq!(
                    paid.player(p).mana().amounts(),
                    plan.left,
                    "plan.left mismatch: {ctx}"
                );
            }
        }
        if expected && !cost.is_zero() {
            payable_count += 1;
            for trial in 0..3 {
                let mut ms = s;
                let mut rem = cost;
                let mut guard = 0;
                while !rem.is_zero() {
                    let opts = mana::manual_options(
                        &ms,
                        &t,
                        p,
                        rem,
                        Bitboard::EMPTY,
                        mana::Paying::NOTHING,
                    )
                    .unwrap();
                    assert!(
                        !opts.is_empty(),
                        "manual_options empty with remaining {rem:?} (trial {trial}): {ctx}"
                    );
                    let pick_ix = match trial {
                        0 => 0,
                        1 => opts.len() - 1,
                        _ => rng.below(opts.len() as u64) as usize,
                    };
                    rem = mana::manual_pay(
                        &mut ms,
                        &t,
                        &mut Sink::off(),
                        p,
                        rem,
                        opts.as_slice()[pick_ix],
                        mana::Paying::NOTHING,
                    )
                    .unwrap()
                    .0;
                    manual_steps += 1;
                    guard += 1;
                    assert!(guard < 40, "manual payment did not terminate: {ctx}");
                }
            }
        }
    }
    assert!(payable_count > 80, "only {payable_count} payable boards");
    assert!(manual_steps > 150, "only {manual_steps} manual steps");
}
/// `ManaChange::Multiplies` with `by < 2`) are refused by the game table.
#[test]
fn table_refuses_unimplemented_mana_changes() {
    static ATTACHED: beryl_card::ManaChange = beryl_card::ManaChange::Adds {
        tapped: beryl_card::Tapped::AttachedTo,
        added: beryl_card::Added::Of(beryl_card::ManaSet::one(ManaColor::Green)),
    };
    static BAD_MULTIPLIER: beryl_card::ManaChange = beryl_card::ManaChange::Multiplies {
        tapped: beryl_card::Filter::Types(Types::LAND),
        by: 1,
    };
    static ATTACHED_EFFECT: beryl_card::Effect =
        beryl_card::effect::build::change_mana(&ATTACHED, beryl_card::Duration::EndOfTurn);
    static BAD_MULTIPLIER_EFFECT: beryl_card::Effect =
        beryl_card::effect::build::change_mana(&BAD_MULTIPLIER, beryl_card::Duration::EndOfTurn);
    static C1: CardDef = CardDef {
        name: "Test Attached",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::INSTANT,
            cost: beryl_card::mana("{G}"),
            abilities: &[Ability::spell().effect(&ATTACHED_EFFECT)],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static C2: CardDef = CardDef {
        name: "Test Bad Multiplier Spell",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::INSTANT,
            cost: beryl_card::mana("{G}"),
            abilities: &[Ability::spell().effect(&BAD_MULTIPLIER_EFFECT)],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static C3: CardDef = CardDef {
        name: "Test Bad Multiplier Permanent",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::ENCHANTMENT,
            cost: beryl_card::mana("{G}"),
            abilities: &[Ability::mana_change(&BAD_MULTIPLIER)],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static CARDS: CardTable = CardTable::new(&[&ISLAND, &C1, &C2, &C3]);
    for name in [
        "Test Attached",
        "Test Bad Multiplier Spell",
        "Test Bad Multiplier Permanent",
    ] {
        let deck: &[(u8, &str)] = &[(36, "Test Island"), (4, name)];
        let res = crate::GameTable::new(
            beryl_kernel::format::Format::CONSTRUCTED,
            &CARDS,
            [deck, deck],
        );
        assert_eq!(
            res.err(),
            Some(crate::table::TableError::ManaChange { card: name }),
            "{name} should be refused"
        );
    }
}
static CREATURES_YOU_CONTROL: &[beryl_card::Filter] = &[
    beryl_card::Filter::Types(Types::CREATURE),
    beryl_card::Filter::Whose(&beryl_card::Players::You),
];
static SPEND_AS_ANY_COLOR_RULE: beryl_card::RuleChange = beryl_card::RuleChange {
    players: beryl_card::Players::You,
    what: beryl_card::Governed::SpendAsAnyColor(beryl_card::Filter::All(CREATURES_YOU_CONTROL)),
    when: beryl_card::When::AsUsual,
};
static DRAW_ONE_EFFECT: beryl_card::Effect = draw(you(), 1);
static COLOR_CAULDRON: CardDef = CardDef {
    name: "Test Color Cauldron",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::rule_change(&SPEND_AS_ANY_COLOR_RULE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static COLORED_CREATURE: CardDef = CardDef {
    name: "Test White Activator",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        abilities: &[
            // Ability 0: {W}: Draw a card.
            Ability::activated(Cost::mana(ManaCost::new(0, [1, 0, 0, 0, 0, 0])))
                .effect(&DRAW_ONE_EFFECT),
            // Ability 1: {W/B}: Draw a card.
            Ability::activated(Cost::mana(ManaCost {
                hybrid: beryl_card::Hybrid::NONE.with_count(
                    beryl_card::Hybrid::pair(
                        (1 << ManaColor::White.index()) | (1 << ManaColor::Black.index()),
                    )
                    .unwrap(),
                    1,
                ),
                ..ManaCost::NONE
            }))
            .effect(&DRAW_ONE_EFFECT),
            // Ability 2: {C}: Draw a card.
            Ability::activated(Cost::mana(ManaCost::new(0, [0, 0, 0, 0, 0, 1])))
                .effect(&DRAW_ONE_EFFECT),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static COLORED_ARTIFACT: CardDef = CardDef {
    name: "Test White Relic",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::activated(Cost::mana(ManaCost::new(0, [1, 0, 0, 0, 0, 0])))
                .effect(&DRAW_ONE_EFFECT),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static WHITE_SPELL: CardDef = CardDef {
    name: "Test White Cantrip",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        cost: ManaCost::new(0, [1, 0, 0, 0, 0, 0]),
        abilities: &[Ability::spell().effect(&DRAW_ONE_EFFECT)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static WASTES: CardDef = CardDef {
    name: "Test Wastes",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::tap_for_mana(crate::card::ManaSet {
            colors: 1 << ManaColor::Colorless.index(),
            amount: 1,
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[test]
fn spend_as_any_color_covers_matching_activations_only() {
    use beryl_card::ObjectAbility;

    static CARDS: CardTable = CardTable::new(&[
        &COLOR_CAULDRON,
        &COLORED_CREATURE,
        &COLORED_ARTIFACT,
        &WHITE_SPELL,
        &ISLAND,
        &WASTES,
    ]);
    let deck: &[(u8, &str)] = &[
        (4, "Test Color Cauldron"),
        (4, "Test White Activator"),
        (4, "Test White Relic"),
        (4, "Test White Cantrip"),
        (4, "Test Wastes"),
        (20, "Test Island"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 7, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let me = s.turn().starting_player();
    let them = s.opponents_of(me).in_seat_order().next().unwrap();

    let cauldron = super::chosen::to_hand(&t, &mut s, me, "Test Color Cauldron");
    let creature = super::chosen::to_hand(&t, &mut s, me, "Test White Activator");
    let relic = super::chosen::to_hand(&t, &mut s, me, "Test White Relic");
    let spell = super::chosen::to_hand(&t, &mut s, me, "Test White Cantrip");
    let island = super::chosen::to_hand(&t, &mut s, me, "Test Island");
    let opp_creature = super::chosen::to_hand(&t, &mut s, them, "Test White Activator");
    let opp_island = super::chosen::to_hand(&t, &mut s, them, "Test Island");

    for slot in [cauldron, creature, relic, island, opp_creature, opp_island] {
        s.board().place(slot, Zone::Battlefield).unwrap();
        s.board().set_sick(slot, false);
    }
    s.board().set_step(TurnStep::PrecombatMain);
    s.board().set_priority(me);
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();

    let mut x = crate::Scratch::default();
    let options = |s: &mut crate::State, x: &mut crate::Scratch| -> Vec<Opt> {
        let crate::Step::Decide(d) = crate::next(s, &t, x, &mut crate::Replay) else {
            panic!("expected priority decision");
        };
        (0..d.len())
            .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
            .collect()
    };
    let opts = options(&mut s, &mut x);
    let act = |slot, i| Opt::Activate {
        slot,
        ability: ObjectAbility::Own(beryl_card::AbilityIx::new(i)),
    };
    let act_white = act(creature, 0);
    let act_hybrid = act(creature, 1);
    let act_colorless = act(creature, 2);

    assert!(opts.contains(&act_white), "{{W}} payable with an Island");
    assert!(opts.contains(&act_hybrid), "{{W/B}} payable with an Island");
    assert!(!opts.contains(&act_colorless), "{{C}} needs colorless mana");
    assert!(!opts.contains(&act(relic, 0)), "noncreature not covered");
    assert!(!opts.contains(&cast(spell)), "spell is not covered");

    // Activating the creature's {W} ability taps the Island and puts it on the stack.
    let pick = opts.iter().position(|o| *o == act_white).unwrap();
    let mut paid = s;
    crate::apply(
        &mut paid,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(pick as u16),
    );
    assert!(paid.object(island).is_tapped());
    assert_eq!(paid.stack().len(), 1);

    // With colorless mana (Wastes), the creature's {C} ability is payable.
    let mut with_wastes = s;
    let wastes = super::chosen::to_hand(&t, &mut with_wastes, me, "Test Wastes");
    with_wastes
        .board()
        .place(wastes, Zone::Battlefield)
        .unwrap();
    crate::continuous::layers::refresh(&mut with_wastes, &t, &mut Sink::off()).unwrap();
    assert!(options(&mut with_wastes, &mut x).contains(&act_colorless));

    // The opponent's creature gets nothing from `me`'s Cauldron.
    s.board().set_priority(them);
    crate::reference::check_references(&s, &t).unwrap();
    if let crate::Step::Decide(d_opp) = crate::next(&mut s, &t, &mut x, &mut crate::Replay)
        && d_opp.player == them
    {
        let opp_opts: Vec<_> = (0..d_opp.len())
            .filter_map(|i| d_opp.candidates.option(crate::Choice(i as u16)))
            .collect();
        assert!(!opp_opts.contains(&act(opp_creature, 0)));
    }
}

static TEST_FLARE_CHANGE: beryl_card::ManaChange = beryl_card::ManaChange::Adds {
    tapped: beryl_card::Tapped::A(beryl_card::Filter::Types(Types::LAND)),
    added: beryl_card::Added::TypeProduced,
};

static TEST_WILD_GROWTH_CHANGE: beryl_card::ManaChange = beryl_card::ManaChange::Adds {
    tapped: beryl_card::Tapped::AttachedTo,
    added: beryl_card::Added::Of(beryl_card::ManaSet::one(ManaColor::Green)),
};

static TEST_FLARE_CREATURE: CardDef = CardDef {
    name: "Test Flare Creature",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        cost: beryl_card::mana("{2}{R}"),
        power: Some(2),
        toughness: Some(2),
        abilities: &[Ability::mana_change(&TEST_FLARE_CHANGE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_WILD_GROWTH_AURA: CardDef = CardDef {
    name: "Test Wild Growth",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: beryl_card::SubtypeWords::new(beryl_card::Subtypes::of(&[
            beryl_card::Subtype::Aura,
        ])),
        cost: beryl_card::mana("{G}"),
        abilities: &[
            Ability::spell().target(&beryl_card::TargetSpec::permanent(
                beryl_card::Filter::Types(Types::LAND),
            )),
            Ability::mana_change(&TEST_WILD_GROWTH_CHANGE),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_WU_TERMS: [beryl_card::Term; 2] = [
    beryl_card::Term::one(ManaColor::White),
    beryl_card::Term::one(ManaColor::Blue),
];

static TEST_CHANCERY: CardDef = CardDef {
    name: "Test Chancery",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(Cost::TAP, &TEST_WU_TERMS)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature with a permanent mana-change ability that lost its abilities adds nothing.
#[test]
fn permanent_mana_change_that_lost_abilities_adds_nothing() {
    static CARDS: CardTable = CardTable::new(&[&ISLAND, &TEST_FLARE_CREATURE]);
    let deck: &[(u8, &str)] = &[(36, "Test Island"), (4, "Test Flare Creature")];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let island = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Island")
        .unwrap();
    let creature = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Flare Creature")
        .unwrap();
    s.board().place(island, Zone::Battlefield).unwrap();
    s.board().place(creature, Zone::Battlefield).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    let two_blue = ManaCost::new(0, [0, 2, 0, 0, 0, 0]);
    assert!(s.has_mana_changes());
    assert!(mana::Funds::of(&s, &t, p).unwrap().pays_for(
        &s,
        &t,
        two_blue,
        Bitboard::EMPTY,
        mana::Paying::NOTHING
    ));
    // Remove all abilities from the creature.
    let card = s.definition(creature).record(&t).0;
    let creature_perm = s.permanent_at(creature).unwrap();
    s.start_effect(
        card,
        crate::state::Continuous::Layer(creature_perm, crate::state::LayerChange::LoseAllAbilities),
        crate::state::Duration::EndOfTurn,
    )
    .unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    assert!(!s.has_mana_changes());
    assert!(!mana::Funds::of(&s, &t, p).unwrap().pays_for(
        &s,
        &t,
        two_blue,
        Bitboard::EMPTY,
        mana::Paying::NOTHING
    ));
    assert_eq!(
        crate::reference::reference_sequences(&s, &t, p, two_blue),
        Some(false)
    );
}

/// Activating a land that produces two types of mana at priority under Mana Flare asks the
/// player to choose one of the produced types, and adds one mana of the chosen type.
#[test]
fn mana_flare_two_type_land_asks_choice_at_priority() {
    static CARDS: CardTable = CardTable::new(&[&TEST_CHANCERY, &TEST_FLARE_CREATURE]);
    let deck: &[(u8, &str)] = &[(36, "Test Chancery"), (4, "Test Flare Creature")];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let mut profile = Profile::EXACT;
    profile.taps_mana = Some(p);
    let mut s = crate::State::new(&t, 1, profile).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let chancery = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Chancery")
        .unwrap();
    let flare = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Flare Creature")
        .unwrap();
    s.board().place(chancery, Zone::Battlefield).unwrap();
    s.board().place(flare, Zone::Battlefield).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    let mut x = crate::Scratch::default();
    let crate::Step::Decide(d) = crate::next(&mut s, &t, &mut x, &mut crate::Replay) else {
        panic!("expected priority decision");
    };
    let act_ix = (0..d.len() as u16)
        .map(crate::Choice)
        .find(|&c| {
            matches!(
                d.candidates.option(c),
                Some(crate::Opt::Activate { slot, .. }) if slot == chancery
            )
        })
        .expect("chancery activation offered at priority");
    crate::apply(&mut s, &t, &mut x, &mut crate::Replay, act_ix);
    let crate::Step::Decide(choose) = crate::next(&mut s, &t, &mut x, &mut crate::Replay) else {
        panic!("expected mana type choice");
    };
    assert_eq!(choose.kind, beryl_card::DecisionKind::ChooseObject);
    assert_eq!(choose.len(), 2, "only White and Blue offered");
    // Pick the second option (Blue).
    crate::apply(&mut s, &t, &mut x, &mut crate::Replay, crate::Choice(1));
    assert_eq!(s.player(p).mana()[ManaColor::White], 1);
    assert_eq!(s.player(p).mana()[ManaColor::Blue], 2);
}

/// On 200 seeded boards with permanent mana-changing abilities (Mana Flare and Wild Growth),
/// the solver's payability, plans (`KeepMostPayable` and `Greedy`), and `Payment::Manual` agree
/// with `reference_sequences`.
#[test]
#[allow(clippy::too_many_lines)]
fn reference_agrees_with_permanent_mana_changes_on_200_seeded_boards() {
    static CARDS: CardTable = CardTable::new(&[
        &LAND,
        &ISLAND,
        &TEST_TUNDRA,
        &TEST_SWAMP,
        &TEST_SIGNET,
        &TEST_CHANCERY,
        &TEST_FLARE_CREATURE,
        &TEST_WILD_GROWTH_AURA,
    ]);
    let deck: &[(u8, &str)] = &[
        (8, "Test Land"),
        (8, "Test Island"),
        (8, "Test Tundra"),
        (8, "Test Swamp"),
        (4, "Test Signet"),
        (4, "Test Chancery"),
        (4, "Test Flare Creature"),
        (4, "Test Wild Growth"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let lands = [
        "Test Land",
        "Test Island",
        "Test Tundra",
        "Test Swamp",
        "Test Signet",
        "Test Chancery",
    ];
    let mut rng = SplitMix64::new(0xF1A2_E907);
    let mut payable_count = 0usize;
    let mut manual_steps = 0usize;
    for board_ix in 0..200 {
        let mut s = crate::State::new(&t, board_ix as u64 + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let n_lands = rng.below(5) as usize + 1;
        let mut placed_lands = Vec::new();
        for _ in 0..n_lands {
            let name = lands[rng.below(lands.len() as u64) as usize];
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == name)
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                if name != "Test Signet" {
                    placed_lands.push(slot);
                }
            }
        }
        // Place 0..=2 Mana Flare creatures (at least one change overall).
        let n_flares = rng.below(3) as usize;
        let mut has_change = false;
        for _ in 0..n_flares {
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == "Test Flare Creature")
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                has_change = true;
            }
        }
        // Attach 0..=2 Wild Growths to placed lands (at least 1 if no Mana Flare).
        let n_growths = if has_change { rng.below(3) as usize } else { 1 };
        if !placed_lands.is_empty() {
            for _ in 0..n_growths {
                if let Some(&aura) = s
                    .library(p)
                    .iter()
                    .find(|&&sl| s.definition_name(&t, sl) == "Test Wild Growth")
                {
                    let host = placed_lands[rng.below(placed_lands.len() as u64) as usize];
                    s.board().place(aura, Zone::Battlefield).unwrap();
                    s.board().attach(aura, host);
                }
            }
        }
        let mut pool = ManaPool::default();
        for color in [
            ManaColor::White,
            ManaColor::Blue,
            ManaColor::Black,
            ManaColor::Green,
        ] {
            pool[color] = rng.below(2) as u8;
        }
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        let cost = ManaCost::new(
            rng.below(4) as u8,
            [
                rng.below(3) as u8,
                rng.below(3) as u8,
                rng.below(2) as u8,
                0,
                rng.below(2) as u8,
                0,
            ],
        );
        let Some(expected) = crate::reference::reference_sequences(&s, &t, p, cost) else {
            continue;
        };
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        let actual = funds.pays_for(&s, &t, cost, Bitboard::EMPTY, mana::Paying::NOTHING);
        assert_eq!(
            actual, expected,
            "board={board_ix} flares={n_flares} growths={n_growths} cost={cost:?}"
        );
        for greedy in [false, true] {
            let plans = mana::plans(
                &s,
                &t,
                &mut Sink::off(),
                p,
                cost,
                Bitboard::EMPTY,
                greedy,
                mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(
                !plans.is_empty(),
                expected,
                "plans mismatch board={board_ix} greedy={greedy} cost={cost:?}"
            );
            for plan in plans.as_slice() {
                let mut paid = s;
                mana::pay(
                    &mut paid,
                    &t,
                    &mut Sink::off(),
                    p,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert_eq!(
                    paid.player(p).mana().amounts(),
                    plan.left,
                    "plan.left mismatch board={board_ix} cost={cost:?}"
                );
            }
        }
        if expected && !cost.is_zero() {
            payable_count += 1;
            for trial in 0..3 {
                let mut ms = s;
                let mut rem = cost;
                let mut guard = 0;
                while !rem.is_zero() {
                    let opts = mana::manual_options(
                        &ms,
                        &t,
                        p,
                        rem,
                        Bitboard::EMPTY,
                        mana::Paying::NOTHING,
                    )
                    .unwrap();
                    assert!(
                        !opts.is_empty(),
                        "manual_options empty board={board_ix} rem={rem:?}"
                    );
                    let pick_ix = match trial {
                        0 => 0,
                        1 => opts.len() - 1,
                        _ => rng.below(opts.len() as u64) as usize,
                    };
                    rem = mana::manual_pay(
                        &mut ms,
                        &t,
                        &mut Sink::off(),
                        p,
                        rem,
                        opts.as_slice()[pick_ix],
                        mana::Paying::NOTHING,
                    )
                    .unwrap()
                    .0;
                    manual_steps += 1;
                    guard += 1;
                    assert!(
                        guard < 40,
                        "manual payment did not terminate board={board_ix}"
                    );
                }
            }
        }
    }
    assert!(payable_count > 80, "only {payable_count} payable boards");
    assert!(manual_steps > 150, "only {manual_steps} manual steps");
}

static TEST_REFLECTION_CHANGE: beryl_card::ManaChange = beryl_card::ManaChange::Multiplies {
    tapped: beryl_card::Filter::Any,
    by: 2,
};
static TEST_MANA_REFLECTION: CardDef = CardDef {
    name: "Test Mana Reflection",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        cost: beryl_card::mana("{4}{G}{G}"),
        abilities: &[Ability::mana_change(&TEST_REFLECTION_CHANGE)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static TEST_LAND_YOU_CONTROL: &[beryl_card::Filter] = &[
    beryl_card::Filter::Types(Types::LAND),
    beryl_card::Filter::Whose(&beryl_card::Players::You),
];
static TEST_LAND_YOU_CONTROL_FILTER: beryl_card::Filter =
    beryl_card::Filter::All(TEST_LAND_YOU_CONTROL);
static TEST_REFLECTING_POOL_TERMS: [beryl_card::Term; 1] = [beryl_card::Term::could_produce(
    &TEST_LAND_YOU_CONTROL_FILTER,
)];
static TEST_REFLECTING_POOL: CardDef = CardDef {
    name: "Test Reflecting Pool",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(
            beryl_card::Cost::TAP,
            &TEST_REFLECTING_POOL_TERMS,
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static TEST_CRYPT: CardDef = CardDef {
    name: "Test Mana Crypt",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::tap_for_mana(beryl_card::ManaSet::of(
            ManaColor::Colorless,
            2,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static TEST_SKYCLOUD_TERMS: [beryl_card::Term; 2] = [
    beryl_card::Term::one(ManaColor::White),
    beryl_card::Term::one(ManaColor::Blue),
];
static TEST_SKYCLOUD: CardDef = CardDef {
    name: "Test Skycloud Expanse",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[Ability::mana_of(
            Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
            &TEST_SKYCLOUD_TERMS,
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Overflowing a multiplier product past `u8::MAX` returns `Full` rather than saturating.
#[test]
fn mana_reflection_overflow_returns_full() {
    static CARDS: CardTable = CardTable::new(&[&ISLAND, &TEST_MANA_REFLECTION, &TEST_CRYPT]);
    let deck: &[(u8, &str)] = &[
        (20, "Test Island"),
        (16, "Test Mana Reflection"),
        (4, "Test Mana Crypt"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    // Eight Mana Reflections give 2^8 = 256, which overflows u8.
    let mut placed = 0;
    for sl in s.library(p).to_vec() {
        if s.definition_name(&t, sl) == "Test Mana Reflection" && placed < 8 {
            s.board().place(sl, Zone::Battlefield).unwrap();
            placed += 1;
        }
    }
    assert_eq!(placed, 8);
    let island = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Island")
        .unwrap();
    s.board().place(island, Zone::Battlefield).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    assert!(
        mana::plans(
            &s,
            &t,
            &mut Sink::off(),
            p,
            ManaCost::new(0, [0, 1, 0, 0, 0, 0]),
            Bitboard::EMPTY,
            false,
            mana::Paying::NOTHING,
        )
        .is_err(),
        "2^8 multiplier must return Full"
    );
}

/// 200 seeded boards mixing Mana Reflection, Reflecting Pool, Mana Crypt, Skycloud Expanse,
/// Tundra, Island, Mana Flare, and tapped/opponent lands agree between `Funds`, `mana::plans`,
/// `mana::pay`, `manual_options`/`manual_pay`, and `reference_sequences`.
#[test]
#[allow(clippy::too_many_lines)]
fn mana_reflection_and_reflecting_pool_agree_with_reference_on_200_seeded_boards() {
    static CARDS: CardTable = CardTable::new(&[
        &ISLAND,
        &TEST_TUNDRA,
        &TEST_CRYPT,
        &TEST_SKYCLOUD,
        &TEST_REFLECTING_POOL,
        &TEST_MANA_REFLECTION,
        &TEST_FLARE_CREATURE,
    ]);
    let deck: &[(u8, &str)] = &[
        (8, "Test Island"),
        (6, "Test Tundra"),
        (4, "Test Mana Crypt"),
        (4, "Test Skycloud Expanse"),
        (6, "Test Reflecting Pool"),
        (6, "Test Mana Reflection"),
        (6, "Test Flare Creature"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut rng = SplitMix64::new(0x005E_ED13_2026);
    let mut payable_count = 0usize;
    let mut manual_steps = 0usize;
    for board_ix in 0..200u64 {
        let mut s = crate::State::new(&t, board_ix + 1000, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let opp = s.opponents_of(p).sole().unwrap();
        let place_some =
            |s: &mut crate::State, seat: beryl_card::Seat, name: &str, count: usize, tap: bool| {
                let mut n = 0;
                for sl in s.library(seat).to_vec() {
                    if n >= count {
                        break;
                    }
                    if s.definition_name(&t, sl) == name {
                        s.board().place(sl, Zone::Battlefield).unwrap();
                        if tap {
                            s.board().set_tapped(sl, true);
                        }
                        n += 1;
                    }
                }
            };
        let n_islands = rng.below(3) as usize;
        let n_tundras = rng.below(2) as usize;
        let n_crypts = rng.below(2) as usize;
        let n_skyclouds = rng.below(2) as usize;
        let n_pools = (rng.below(3) + 1) as usize;
        let n_reflections = rng.below(3) as usize;
        let n_flares = rng.below(2) as usize;
        place_some(&mut s, p, "Test Island", n_islands, rng.below(3) == 0);
        place_some(&mut s, p, "Test Tundra", n_tundras, rng.below(3) == 0);
        place_some(&mut s, p, "Test Mana Crypt", n_crypts, false);
        place_some(&mut s, p, "Test Skycloud Expanse", n_skyclouds, false);
        place_some(&mut s, p, "Test Reflecting Pool", n_pools, false);
        place_some(&mut s, p, "Test Mana Reflection", n_reflections, false);
        place_some(&mut s, p, "Test Flare Creature", n_flares, false);
        // Opponent lands and Mana Reflection should not affect P0's pool or multiplier.
        place_some(&mut s, opp, "Test Tundra", rng.below(2) as usize, false);
        place_some(
            &mut s,
            opp,
            "Test Mana Reflection",
            rng.below(2) as usize,
            false,
        );
        let mut pool = ManaPool::default();
        for color in [ManaColor::White, ManaColor::Blue, ManaColor::Colorless] {
            pool[color] = rng.below(2) as u8;
        }
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        let cost = ManaCost::new(
            rng.below(5) as u8,
            [
                rng.below(3) as u8,
                rng.below(3) as u8,
                0,
                0,
                0,
                rng.below(3) as u8,
            ],
        );
        let Some(expected) = crate::reference::reference_sequences(&s, &t, p, cost) else {
            continue;
        };
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        let actual = funds.pays_for(&s, &t, cost, Bitboard::EMPTY, mana::Paying::NOTHING);
        assert_eq!(
            actual, expected,
            "board={board_ix} refl={n_reflections} pools={n_pools} flares={n_flares} cost={cost:?}"
        );
        for greedy in [false, true] {
            let plans = mana::plans(
                &s,
                &t,
                &mut Sink::off(),
                p,
                cost,
                Bitboard::EMPTY,
                greedy,
                mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(
                !plans.is_empty(),
                expected,
                "plans mismatch board={board_ix} greedy={greedy} cost={cost:?}"
            );
            for plan in plans.as_slice() {
                let mut paid = s;
                mana::pay(
                    &mut paid,
                    &t,
                    &mut Sink::off(),
                    p,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert_eq!(
                    paid.player(p).mana().amounts(),
                    plan.left,
                    "plan.left mismatch board={board_ix} cost={cost:?}"
                );
            }
        }
        if expected && !cost.is_zero() {
            payable_count += 1;
            for trial in 0..2 {
                let mut ms = s;
                let mut rem = cost;
                let mut guard = 0;
                while !rem.is_zero() {
                    let opts = mana::manual_options(
                        &ms,
                        &t,
                        p,
                        rem,
                        Bitboard::EMPTY,
                        mana::Paying::NOTHING,
                    )
                    .unwrap();
                    assert!(
                        !opts.is_empty(),
                        "manual_options empty board={board_ix} rem={rem:?}"
                    );
                    let pick_ix = if trial == 0 { 0 } else { opts.len() - 1 };
                    rem = mana::manual_pay(
                        &mut ms,
                        &t,
                        &mut Sink::off(),
                        p,
                        rem,
                        opts.as_slice()[pick_ix],
                        mana::Paying::NOTHING,
                    )
                    .unwrap()
                    .0;
                    manual_steps += 1;
                    guard += 1;
                    assert!(
                        guard < 40,
                        "manual payment did not terminate board={board_ix}"
                    );
                }
            }
        }
    }
    assert!(payable_count > 80, "only {payable_count} payable boards");
    assert!(manual_steps > 100, "only {manual_steps} manual steps");
}

/// On 200 seeded boards with granted mana abilities (including a noncreature permanent granted a
/// mana ability, summoning-sick creatures, and duplicate grantors), the payment reference agrees
/// with the solver, `Funds`, `plans`, and `Payment::Manual`, and duplicate grantors offer each
/// color once.
#[test]
#[allow(clippy::too_many_lines)]
fn reference_agrees_with_granted_mana_on_200_seeded_boards() {
    use crate::card::{LayerSpec, ManaSet, Modification, Objects, Subtype, Subtypes};
    use beryl_card::{Filter, SubtypeWords};

    static SLIVER_GRANTOR: CardDef = CardDef {
        name: "Test Sliver Grantor",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::CREATURE,
            subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Sliver])),
            power: Some(1),
            toughness: Some(1),
            abilities: &[Ability::static_ability(&LayerSpec {
                affected: Objects::All {
                    zone: Zone::Battlefield,
                    filter: Filter::Subtype(Subtype::Sliver),
                },
                condition: None,
                modifications: &[Modification::grant(0)],
            })],
            grants: &[Ability::tap_for_mana(ManaSet::any_color(1))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static ARTIFACT_GRANTOR: CardDef = CardDef {
        name: "Test Artifact Mana Grantor",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::ENCHANTMENT,
            abilities: &[Ability::static_ability(&LayerSpec {
                affected: Objects::All {
                    zone: Zone::Battlefield,
                    filter: Filter::Types(Types::ARTIFACT),
                },
                condition: None,
                modifications: &[Modification::grant(0)],
            })],
            grants: &[Ability::tap_for_mana(ManaSet::any_color(1))],
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static PLAIN_SLIVER: CardDef = CardDef {
        name: "Test Plain Sliver",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::CREATURE,
            subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Sliver])),
            power: Some(1),
            toughness: Some(1),
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static PLAIN_ARTIFACT: CardDef = CardDef {
        name: "Test Plain Artifact",
        shape: beryl_card::definition::Shape::Single,
        faces: &[Face {
            types: Types::ARTIFACT,
            ..Face::EMPTY
        }],
        tokens: &[],
    };
    static CARDS: CardTable = CardTable::new(&[
        &LAND,
        &ISLAND,
        &BEAR,
        &SLIVER_GRANTOR,
        &ARTIFACT_GRANTOR,
        &PLAIN_SLIVER,
        &PLAIN_ARTIFACT,
    ]);
    let deck: &[(u8, &str)] = &[
        (8, "Test Land"),
        (8, "Test Island"),
        (8, "Test Bear"),
        (4, "Test Sliver Grantor"),
        (4, "Test Artifact Mana Grantor"),
        (8, "Test Plain Sliver"),
        (8, "Test Plain Artifact"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    assert!(t.can(crate::table::Capability::GrantsMana));

    // Verify two grantors offer one standalone mana option per color (5 total) for a Sliver.
    {
        let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        for _ in 0..2 {
            let slot = s
                .library(p)
                .iter()
                .copied()
                .find(|&sl| s.definition_name(&t, sl) == "Test Sliver Grantor")
                .unwrap();
            s.board().place(slot, Zone::Battlefield).unwrap();
            let perm = s.permanent_at(slot).unwrap();
            s.set_summoning_sick(perm, false);
        }
        let sliver = s
            .library(p)
            .iter()
            .copied()
            .find(|&sl| s.definition_name(&t, sl) == "Test Plain Sliver")
            .unwrap();
        s.board().place(sliver, Zone::Battlefield).unwrap();
        let perm = s.permanent_at(sliver).unwrap();
        s.set_summoning_sick(perm, false);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        assert!(!s.makes_card_mana(&t, sliver));
        let mut opts = beryl_kernel::fixed::Fixed::new();
        mana::standalone_options(&s, &t, p, &mut opts);
        let sliver_opts = opts
            .as_slice()
            .iter()
            .filter(|o| matches!(o, Opt::Mana { slot, .. } if *slot == sliver))
            .count();
        assert_eq!(sliver_opts, 5, "two grantors must offer each color once");
    }

    let permanents = [
        "Test Land",
        "Test Island",
        "Test Bear",
        "Test Plain Sliver",
        "Test Plain Artifact",
    ];
    let mut rng = SplitMix64::new(0x6E47_A001);
    let mut payable_count = 0usize;
    let mut manual_steps = 0usize;
    for board_ix in 0..200 {
        let mut s = crate::State::new(&t, board_ix as u64 + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let n_perms = rng.below(5) as usize + 1;
        for _ in 0..n_perms {
            let name = permanents[rng.below(permanents.len() as u64) as usize];
            if let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == name)
            {
                s.board().place(slot, Zone::Battlefield).unwrap();
                let sick = rng.below(2) == 0;
                let perm = s.permanent_at(slot).unwrap();
                s.set_summoning_sick(perm, sick);
            }
        }
        // Place 0..=2 Sliver grantors and 0..=2 Artifact grantors (at least one grantor overall).
        let n_sliver_grantors = rng.below(3) as usize;
        let n_art_grantors = if n_sliver_grantors == 0 {
            rng.below(2) as usize + 1
        } else {
            rng.below(3) as usize
        };
        for (name, count) in [
            ("Test Sliver Grantor", n_sliver_grantors),
            ("Test Artifact Mana Grantor", n_art_grantors),
        ] {
            for _ in 0..count {
                if let Some(&slot) = s
                    .library(p)
                    .iter()
                    .find(|&&sl| s.definition_name(&t, sl) == name)
                {
                    s.board().place(slot, Zone::Battlefield).unwrap();
                    let sick = rng.below(2) == 0;
                    let perm = s.permanent_at(slot).unwrap();
                    s.set_summoning_sick(perm, sick);
                }
            }
        }
        // Ensure at least one noncreature artifact is present on half the boards.
        if board_ix % 2 == 0
            && let Some(&slot) = s
                .library(p)
                .iter()
                .find(|&&sl| s.definition_name(&t, sl) == "Test Plain Artifact")
        {
            s.board().place(slot, Zone::Battlefield).unwrap();
            let perm = s.permanent_at(slot).unwrap();
            s.set_summoning_sick(perm, true);
        }
        let mut pool = ManaPool::default();
        for color in [
            ManaColor::White,
            ManaColor::Blue,
            ManaColor::Black,
            ManaColor::Red,
            ManaColor::Green,
        ] {
            pool[color] = rng.below(2) as u8;
        }
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        crate::reference::check_references(&s, &t).unwrap();

        let cost = ManaCost::new(
            rng.below(3) as u8,
            [
                rng.below(2) as u8,
                rng.below(2) as u8,
                rng.below(2) as u8,
                rng.below(2) as u8,
                rng.below(2) as u8,
                0,
            ],
        );
        let expected = crate::reference::reference_payable(&s, &t, p, cost).unwrap();
        let prefilter =
            mana::payable(&s, &t, p, cost, Bitboard::EMPTY, mana::Paying::NOTHING).unwrap();
        assert_eq!(
            prefilter, expected,
            "prefilter board={board_ix} cost={cost:?}"
        );
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        assert_eq!(
            funds.pays_for(&s, &t, cost, Bitboard::EMPTY, mana::Paying::NOTHING),
            expected,
            "funds board={board_ix} cost={cost:?}"
        );
        for greedy in [false, true] {
            let plans = mana::plans(
                &s,
                &t,
                &mut Sink::off(),
                p,
                cost,
                Bitboard::EMPTY,
                greedy,
                mana::Paying::NOTHING,
            )
            .unwrap();
            assert_eq!(
                !plans.is_empty(),
                expected,
                "plans board={board_ix} greedy={greedy} cost={cost:?}"
            );
            for plan in plans.as_slice() {
                let mut paid = s;
                mana::pay(
                    &mut paid,
                    &t,
                    &mut Sink::off(),
                    p,
                    cost,
                    Bitboard::EMPTY,
                    plan,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert_eq!(
                    paid.player(p).mana().amounts(),
                    plan.left,
                    "plan.left board={board_ix} cost={cost:?}"
                );
            }
        }
        if expected && !cost.is_zero() {
            payable_count += 1;
            let mut ms = s;
            let mut rem = cost;
            let mut guard = 0;
            while !rem.is_zero() {
                let opts =
                    mana::manual_options(&ms, &t, p, rem, Bitboard::EMPTY, mana::Paying::NOTHING)
                        .unwrap();
                assert!(
                    !opts.is_empty(),
                    "manual_options empty board={board_ix} rem={rem:?}"
                );
                let pick_ix = rng.below(opts.len() as u64) as usize;
                rem = mana::manual_pay(
                    &mut ms,
                    &t,
                    &mut Sink::off(),
                    p,
                    rem,
                    opts.as_slice()[pick_ix],
                    mana::Paying::NOTHING,
                )
                .unwrap()
                .0;
                manual_steps += 1;
                guard += 1;
                assert!(
                    guard < 40,
                    "manual payment did not terminate board={board_ix}"
                );
            }
        }
    }
    assert!(payable_count > 60, "only {payable_count} payable boards");
    assert!(manual_steps > 100, "only {manual_steps} manual steps");
}

static TEST_ONE_SPELL: CardDef = CardDef {
    name: "Test One Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{1}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_WG_SPELL: CardDef = CardDef {
    name: "Test WG Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{W}{G}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_WU_SPELL: CardDef = CardDef {
    name: "Test WU Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{W}{U}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TEST_U_SPELL: CardDef = CardDef {
    name: "Test U Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{U}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// `{U}, {T}: Add {W}{G}.`
static TEST_U_TO_WG_SIGNET: CardDef = CardDef {
    name: "Test U-to-WG Signet",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana_of(
            Cost::of(beryl_card::mana("{U}"), &[CostPart::TAP]),
            &[
                beryl_card::Term::one(ManaColor::White),
                beryl_card::Term::one(ManaColor::Green),
            ],
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[test]
#[allow(clippy::too_many_lines)]
fn priced_source_at_priority_offered_while_price_payable_and_activates() {
    static CARDS: CardTable = CardTable::new(&[&LAND, &ISLAND, &TEST_SIGNET, &TEST_WU_SPELL]);
    let deck: &[(u8, &str)] = &[
        (15, "Test Land"),
        (15, "Test Island"),
        (15, "Test Signet"),
        (15, "Test WU Spell"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    s.set_profile(Profile {
        taps_mana: Some(p),
        ..Profile::EXACT
    });
    let hand = s
        .zone(beryl_kernel::state::ZoneKey::Of(p, Zone::Hand))
        .fold(Vec::new(), |mut v, sl| {
            v.push(sl);
            v
        });
    for sl in hand {
        s.board().place(sl, Zone::Library).unwrap();
    }
    let land = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Land")
        .unwrap();
    let island = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Island")
        .unwrap();
    let signet = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Signet")
        .unwrap();
    let spell = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test WU Spell")
        .unwrap();
    let land_in_hand = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| sl != land && s.definition_name(&t, sl) == "Test Land")
        .unwrap();
    s.board().place(signet, Zone::Battlefield).unwrap();
    s.board().place(land_in_hand, Zone::Hand).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();

    let signet_opt = Opt::Activate {
        slot: signet,
        ability: beryl_card::ObjectAbility::Own(beryl_card::AbilityIx::new(0)),
    };
    let options_of = |st: &mut crate::State| {
        let mut x = crate::Scratch::default();
        let crate::Step::Decide(d) = crate::next(st, &t, &mut x, &mut crate::Replay) else {
            panic!("expected decision");
        };
        (
            d.player,
            d.kind,
            (0..d.len())
                .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
                .collect::<Vec<_>>(),
        )
    };

    // Without an untapped land, the Signet's {1} price is unpayable so it is not offered.
    let (who, kind, opts) = options_of(&mut s);
    assert_eq!((who, kind), (p, DecisionKind::Priority));
    assert!(!opts.contains(&signet_opt), "{opts:?}");

    // With an untapped land, the Signet is offered at priority.
    s.board().place(land, Zone::Battlefield).unwrap();
    s.board().place(spell, Zone::Hand).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
    let (_, _, opts) = options_of(&mut s);
    assert!(opts.contains(&signet_opt), "{opts:?}");

    // Activating it pays {1} by tapping the land and leaves {W}{U} floating with priority kept.
    let mut manual_s = s;
    let mut x = crate::Scratch::default();
    let crate::Step::Decide(d) = crate::next(&mut s, &t, &mut x, &mut crate::Replay) else {
        panic!("expected priority decision");
    };
    let choice = (0..d.len())
        .find(|&i| d.candidates.option(crate::Choice(i as u16)) == Some(signet_opt))
        .unwrap();
    crate::apply(
        &mut s,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(choice as u16),
    );
    assert!(s.object(land).is_tapped());
    assert!(s.object(signet).is_tapped());
    assert_eq!(s.player(p).mana().amounts(), [1, 1, 0, 0, 0, 0]);
    assert_eq!(s.turn().priority(), p);

    // Under Payment::Manual with two distinct lands, activating it asks ManaPayment to choose
    // which source pays the {1} price, without offering the Signet itself.
    manual_s.board().place(island, Zone::Battlefield).unwrap();
    crate::continuous::layers::refresh(&mut manual_s, &t, &mut Sink::off()).unwrap();
    manual_s.set_profile(Profile {
        payment: crate::Payment::Manual,
        ..Profile::EXACT
    });
    let crate::Step::Decide(d) = crate::next(&mut manual_s, &t, &mut x, &mut crate::Replay) else {
        panic!("expected priority decision");
    };
    let choice = (0..d.len())
        .find(|&i| d.candidates.option(crate::Choice(i as u16)) == Some(signet_opt))
        .unwrap();
    crate::apply(
        &mut manual_s,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(choice as u16),
    );
    let crate::Step::Decide(pay_d) = crate::next(&mut manual_s, &t, &mut x, &mut crate::Replay)
    else {
        panic!("expected mana payment decision");
    };
    assert_eq!(pay_d.kind, DecisionKind::ManaPayment);
    let pay_opts: Vec<_> = (0..pay_d.len())
        .filter_map(|i| pay_d.candidates.option(crate::Choice(i as u16)))
        .collect();
    assert!(!pay_opts.contains(&signet_opt), "{pay_opts:?}");
    let tap_land = pay_opts
        .iter()
        .position(|&o| {
            o == Opt::Mana {
                slot: land,
                color: ManaColor::White,
            }
        })
        .unwrap();
    crate::apply(
        &mut manual_s,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(tap_land as u16),
    );
    assert!(manual_s.object(land).is_tapped());
    assert!(!manual_s.object(island).is_tapped());
    assert!(manual_s.object(signet).is_tapped());
    assert_eq!(manual_s.player(p).mana().amounts(), [1, 1, 0, 0, 0, 0]);
}

#[test]
fn priced_source_not_offered_without_taps_mana_and_idle_priority_passes_silently() {
    static CARDS: CardTable = CardTable::new(&[&LAND, &TEST_SIGNET, &TEST_WU_SPELL]);
    let deck: &[(u8, &str)] = &[
        (20, "Test Land"),
        (20, "Test Signet"),
        (20, "Test WU Spell"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    let hand = s
        .zone(beryl_kernel::state::ZoneKey::Of(p, Zone::Hand))
        .fold(Vec::new(), |mut v, sl| {
            v.push(sl);
            v
        });
    for sl in hand {
        s.board().place(sl, Zone::Library).unwrap();
    }
    let land = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Land")
        .unwrap();
    let signet = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test Signet")
        .unwrap();
    let spell = s
        .library(p)
        .iter()
        .copied()
        .find(|&sl| s.definition_name(&t, sl) == "Test WU Spell")
        .unwrap();
    s.board().place(land, Zone::Battlefield).unwrap();
    s.board().place(signet, Zone::Battlefield).unwrap();
    crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();

    // Without taps_mana (with a castable spell so priority stops), the Signet is not offered.
    let mut with_spell = s;
    with_spell.board().place(spell, Zone::Hand).unwrap();
    crate::continuous::layers::refresh(&mut with_spell, &t, &mut Sink::off()).unwrap();
    let mut x = crate::Scratch::default();
    let crate::Step::Decide(d) = crate::next(&mut with_spell, &t, &mut x, &mut crate::Replay)
    else {
        panic!("expected priority decision");
    };
    let signet_opt = Opt::Activate {
        slot: signet,
        ability: beryl_card::ObjectAbility::Own(beryl_card::AbilityIx::new(0)),
    };
    let opts: Vec<_> = (0..d.len())
        .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
        .collect();
    assert!(!opts.contains(&signet_opt), "{opts:?}");

    // With taps_mana and nothing to spend mana on, only_idle passes silently.
    s.set_profile(Profile {
        taps_mana: Some(p),
        ..Profile::EXACT
    });
    let crate::Step::Decide(d) = crate::next(&mut s, &t, &mut x, &mut crate::Replay) else {
        panic!("expected decision");
    };
    let offers_signet =
        (0..d.len()).any(|i| d.candidates.option(crate::Choice(i as u16)) == Some(signet_opt));
    assert!(!(d.player == p && offers_signet), "{d:?}");
    assert!(!s.object(land).is_tapped());
    assert!(!s.object(signet).is_tapped());
}

#[test]
#[allow(clippy::too_many_lines)]
fn lookahead_keeps_later_two_color_spell_payable_through_priced_source() {
    static CARDS: CardTable = CardTable::new(&[
        &ISLAND,
        &TEST_SWAMP,
        &TEST_U_TO_WG_SIGNET,
        &TEST_ONE_SPELL,
        &TEST_WG_SPELL,
        &TEST_SIGNET,
        &TEST_WU_SPELL,
        &TEST_U_SPELL,
    ]);
    let deck: &[(u8, &str)] = &[
        (10, "Test Island"),
        (10, "Test Swamp"),
        (8, "Test U-to-WG Signet"),
        (8, "Test One Spell"),
        (8, "Test WG Spell"),
        (6, "Test Signet"),
        (5, "Test WU Spell"),
        (5, "Test U Spell"),
    ];
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap();
    let mut s = crate::State::new(&t, 1, Profile::EXACT).unwrap();
    super::begin(&t, &mut s);
    let p = s.turn().starting_player();
    s.set_profile(Profile {
        taps_mana: Some(p),
        ..Profile::EXACT
    });
    let hand = s
        .zone(beryl_kernel::state::ZoneKey::Of(p, Zone::Hand))
        .fold(Vec::new(), |mut v, sl| {
            v.push(sl);
            v
        });
    for sl in hand {
        s.board().place(sl, Zone::Library).unwrap();
    }
    let find_lib = |st: &crate::State, name: &str| {
        st.library(p)
            .iter()
            .copied()
            .find(|&sl| st.definition_name(&t, sl) == name)
            .unwrap()
    };

    // Case 1: Island + Swamp + `{U}, {T}: Add {W}{G}` with `{1}` and `{W}{G}` in hand.
    // Casting `{1}` must tap Swamp so Island can still pay for the Signet to cast `{W}{G}`.
    let mut s1 = s;
    let island = find_lib(&s1, "Test Island");
    let swamp = find_lib(&s1, "Test Swamp");
    let u_signet = find_lib(&s1, "Test U-to-WG Signet");
    let one_spell = find_lib(&s1, "Test One Spell");
    let wg_spell = find_lib(&s1, "Test WG Spell");
    s1.board().place(island, Zone::Battlefield).unwrap();
    s1.board().place(swamp, Zone::Battlefield).unwrap();
    s1.board().place(u_signet, Zone::Battlefield).unwrap();
    s1.board().place(one_spell, Zone::Hand).unwrap();
    s1.board().place(wg_spell, Zone::Hand).unwrap();
    crate::continuous::layers::refresh(&mut s1, &t, &mut Sink::off()).unwrap();

    let mut x = crate::Scratch::default();
    let crate::Step::Decide(d) = crate::next(&mut s1, &t, &mut x, &mut crate::Replay) else {
        panic!("expected priority decision");
    };
    let cast_one = (0..d.len())
        .find(|&i| d.candidates.option(crate::Choice(i as u16)) == Some(cast(one_spell)))
        .unwrap();
    crate::apply(
        &mut s1,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(cast_one as u16),
    );
    assert!(s1.object(swamp).is_tapped(), "should tap Swamp for {{1}}");
    assert!(
        !s1.object(island).is_tapped(),
        "should keep Island untapped for the priced source"
    );
    assert!(!s1.object(u_signet).is_tapped());
    assert!(
        mana::payable(
            &s1,
            &t,
            p,
            beryl_card::mana("{W}{G}"),
            Bitboard::EMPTY,
            mana::Paying::NOTHING
        )
        .unwrap()
    );

    // Case 2: Island + Swamp + `{1}, {T}: Add {W}{U}` with `{W}{U}` and `{U}` in hand.
    // Casting `{W}{U}` through the Signet must tap Swamp for the Signet's `{1}`, keeping Island
    // untapped for the `{U}` spell.
    let mut s2 = s;
    let island = find_lib(&s2, "Test Island");
    let swamp = find_lib(&s2, "Test Swamp");
    let signet = find_lib(&s2, "Test Signet");
    let azorius_spell = find_lib(&s2, "Test WU Spell");
    let u_spell = find_lib(&s2, "Test U Spell");
    s2.board().place(island, Zone::Battlefield).unwrap();
    s2.board().place(swamp, Zone::Battlefield).unwrap();
    s2.board().place(signet, Zone::Battlefield).unwrap();
    s2.board().place(azorius_spell, Zone::Hand).unwrap();
    s2.board().place(u_spell, Zone::Hand).unwrap();
    crate::continuous::layers::refresh(&mut s2, &t, &mut Sink::off()).unwrap();

    let crate::Step::Decide(d) = crate::next(&mut s2, &t, &mut x, &mut crate::Replay) else {
        panic!("expected priority decision");
    };
    let cast_wu = (0..d.len())
        .find(|&i| d.candidates.option(crate::Choice(i as u16)) == Some(cast(azorius_spell)))
        .unwrap();
    crate::apply(
        &mut s2,
        &t,
        &mut x,
        &mut crate::Replay,
        crate::Choice(cast_wu as u16),
    );
    assert!(s2.object(swamp).is_tapped(), "should tap Swamp for Signet");
    assert!(s2.object(signet).is_tapped(), "should tap Signet");
    assert!(
        !s2.object(island).is_tapped(),
        "should keep Island untapped for {{U}} spell"
    );
}
