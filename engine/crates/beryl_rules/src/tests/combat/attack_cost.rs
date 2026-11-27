//! Costs to attack, as "Creatures can't attack you unless their controller pays {2} for each
//! creature they control that's attacking you." makes one, with test-only cards: which
//! attackers are offered, the payment after the attackers tap, and the independent enumerating
//! reference (`crate::reference::verify_all_paths`) on seeded boards.

use super::Game;
use crate::card::{
    Ability, CardDef, Face, Keywords, ManaCost, ManaSet, Objects, Players, RuleChange, Types,
};
use crate::profile::{Breakpoint, Payment};
use crate::reference::verify_all_paths;
use crate::state::{AttackTarget, Pending};
use crate::{
    Candidates, CardTable, Choice, Decision, DecisionKind, Decklist, GameTable, Opt, Profile,
    Replay, Step, TurnStep, apply, next,
};
use beryl_card::{Governed, When};
use beryl_kernel::format::Format;

static ATTACK_COST: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::AttackCost(ManaCost::generic(2)),
    when: When::AsUsual,
};
static MUST_ATTACK: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::MustAttack(Objects::This),
    when: When::AsUsual,
};
static ALONE: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::Alone {
        attack: true,
        block: true,
        creatures: Objects::This,
    },
    when: When::AsUsual,
};

/// Blue mana, which the lands and the mana creatures make.
const BLUE: ManaSet = ManaSet {
    colors: 1 << 1,
    amount: 1,
};
/// Red mana, which the second kind of land makes.
const RED: ManaSet = ManaSet {
    colors: 1 << 3,
    amount: 1,
};

macro_rules! card {
    ($ident:ident, $name:literal, $types:expr, $pt:expr, $keywords:expr, $abilities:expr) => {
        static $ident: CardDef = CardDef {
            name: $name,
            shape: beryl_card::definition::Shape::Single,
            faces: &[Face {
                types: $types,
                power: $pt,
                toughness: $pt,
                keywords: $keywords,
                abilities: $abilities,
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

card!(
    TOLL,
    "Toll",
    Types::ENCHANTMENT,
    None,
    Keywords::NONE,
    &[Ability::rule_change(&ATTACK_COST)]
);
card!(
    ISLE,
    "Isle",
    Types::LAND,
    None,
    Keywords::NONE,
    &[Ability::tap_for_mana(BLUE)]
);
card!(
    PEAK,
    "Peak",
    Types::LAND,
    None,
    Keywords::NONE,
    &[Ability::tap_for_mana(RED)]
);
card!(
    PLAIN,
    "Plain",
    Types::CREATURE,
    Some(2),
    Keywords::NONE,
    &[]
);
card!(
    DORK,
    "Dork",
    Types::CREATURE,
    Some(1),
    Keywords::NONE,
    &[Ability::tap_for_mana(BLUE)]
);
card!(
    WATCHER,
    "Watcher",
    Types::CREATURE,
    Some(1),
    Keywords::VIGILANCE,
    &[Ability::tap_for_mana(BLUE)]
);
card!(
    RECKLESS,
    "Reckless",
    Types::CREATURE,
    Some(2),
    Keywords::NONE,
    &[Ability::rule_change(&MUST_ATTACK)]
);
card!(
    SHY,
    "Shy",
    Types::CREATURE,
    Some(2),
    Keywords::NONE,
    &[Ability::rule_change(&ALONE)]
);

static WALKER: CardDef = CardDef {
    name: "Walker",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &TOLL, &ISLE, &PEAK, &PLAIN, &DORK, &WATCHER, &RECKLESS, &SHY, &WALKER,
]);

const DECK: &Decklist = &[
    (2, "Toll"),
    (6, "Isle"),
    (2, "Peak"),
    (3, "Plain"),
    (3, "Dork"),
    (3, "Watcher"),
    (3, "Reckless"),
    (3, "Shy"),
    (1, "Walker"),
];

fn game(seed: u64) -> Game {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).expect("attack cost table");
    Game::with_table(t, seed)
}

/// Passes priority until the attacking player is asked to declare attackers.
fn to_attackers(g: &mut Game) -> Decision {
    loop {
        match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
            Step::Decide(d) if d.kind == DecisionKind::Priority => {
                apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
            }
            Step::Decide(d) => {
                assert_eq!(d.kind, DecisionKind::Attacker, "{d:?}");
                return d;
            }
            Step::Over(o) => panic!("the game ended: {o:?}"),
        }
    }
}

fn options(d: &Decision) -> Vec<Opt> {
    (0..d.len() as u16)
        .map(|i| d.candidates.option(Choice(i)).expect("an option"))
        .collect()
}

fn choose(g: &mut Game, d: &Decision, opt: Opt) {
    let i = options(d)
        .iter()
        .position(|&o| o == opt)
        .unwrap_or_else(|| panic!("{opt:?} not offered: {d:?}"));
    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(i as u16));
}

fn tapped(g: &Game, slot: crate::ids::Slot) -> bool {
    g.s.object(slot).is_tapped()
}

/// An attacker with vigilance isn't tapped by attacking, so it can tap for mana to pay its own
/// cost to attack (see CR 508.1f, CR 702.20b).
#[test]
fn an_attacker_with_vigilance_that_taps_for_mana_pays_for_its_own_attack() {
    let mut g = game(1);
    let watcher = g.put(g.attacker, "Watcher");
    let isle = g.put(g.attacker, "Isle");
    g.put(g.defender(), "Toll");
    let d = to_attackers(&mut g);
    assert_eq!(options(&d), [Opt::Done, Opt::attack(watcher)]);
    choose(&mut g, &d, Opt::attack(watcher));
    g.finish_combat();
    assert!(tapped(&g, watcher) && tapped(&g, isle));
    assert_eq!(g.life(g.defender()), 19);
}

/// A creature that must attack isn't required to while attacking costs mana (see CR 508.1d):
/// Done is offered, and so is attacking with it and paying.
#[test]
fn a_creature_that_must_attack_is_not_forced_to_pay_and_may() {
    for attacks in [false, true] {
        let mut g = game(2);
        let reckless = g.put(g.attacker, "Reckless");
        let isles = [g.put(g.attacker, "Isle"), g.put(g.attacker, "Isle")];
        g.put(g.defender(), "Toll");
        let d = to_attackers(&mut g);
        assert_eq!(options(&d), [Opt::Done, Opt::attack(reckless)]);
        choose(
            &mut g,
            &d,
            if attacks {
                Opt::attack(reckless)
            } else {
                Opt::Done
            },
        );
        g.finish_combat();
        assert_eq!(isles.map(|isle| tapped(&g, isle)), [attacks; 2]);
        assert_eq!(g.life(g.defender()), if attacks { 18 } else { 20 });
    }
}

/// Under manual payment the cost to attack is paid one source at a time, after the attackers
/// have tapped, and the attack is declared once it is all paid.
#[test]
fn under_manual_payment_the_cost_to_attack_is_paid_one_source_at_a_time() {
    let mut g = game(3);
    let plain = g.put(g.attacker, "Plain");
    let isle = g.put(g.attacker, "Isle");
    let peak = g.put(g.attacker, "Peak");
    g.put(g.defender(), "Toll");
    g.s.set_profile(Profile {
        payment: Payment::Manual,
        breakpoint: Some(Breakpoint {
            turn: 1,
            step: TurnStep::PostcombatMain,
        }),
        ..Profile::EXACT
    });
    let d = to_attackers(&mut g);
    // The one creature is chosen, and Done follows without asking.
    choose(&mut g, &d, Opt::attack(plain));
    let d = match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    };
    assert_eq!(d.kind, DecisionKind::ManaPayment, "{d:?}");
    assert!(tapped(&g, plain), "the attacker taps before the payment");
    assert!(!tapped(&g, isle) && !tapped(&g, peak));
    let isle_mana = options(&d)
        .into_iter()
        .find(|o| matches!(o, Opt::Mana { slot, .. } if *slot == isle))
        .expect("the Isle is offered");
    choose(&mut g, &d, isle_mana);
    assert!(tapped(&g, isle) && !tapped(&g, peak));
    assert_eq!(
        *g.s.pending(),
        Pending::PayingToAttack {
            remaining: ManaCost::generic(1),
        }
    );
    g.finish_combat();
    assert!(tapped(&g, peak));
    assert_eq!(g.life(g.defender()), 18);
}

fn next_rng(rng: &mut u64) -> u64 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

/// On seeded boards of creatures that make mana, with and without vigilance, creatures that
/// must attack or can't attack alone, lands, one or two costs to attack and sometimes a
/// planeswalker to attack instead: the attackers offered are exactly those of the legal
/// declarations the enumerating reference finds, and any declaration played to its end is
/// paid for.
#[test]
fn no_attacker_is_offered_whose_cost_cannot_be_paid_on_200_seeded_boards() {
    const CREATURES: [&str; 5] = ["Plain", "Dork", "Watcher", "Reckless", "Shy"];
    let mut declared = 0;
    for seed in 0..200u64 {
        let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        let mut g = game(seed);
        let (active, defender) = (g.attacker, g.defender());
        let mut counts = [0usize; CREATURES.len()];
        for _ in 0..=next_rng(&mut rng) % 4 {
            let mut i = (next_rng(&mut rng) as usize) % CREATURES.len();
            while counts[i] >= 3 {
                i = (i + 1) % CREATURES.len();
            }
            counts[i] += 1;
            g.put(active, CREATURES[i]);
        }
        for _ in 0..next_rng(&mut rng) % 6 {
            g.put(active, "Isle");
        }
        for _ in 0..=next_rng(&mut rng) % 2 {
            g.put(defender, "Toll");
        }
        if next_rng(&mut rng).is_multiple_of(3) {
            g.put(defender, "Walker");
        }
        // A board with one pick asks nothing: Done for a lone creature that can't attack alone
        // or has no mana for the cost, or the one creature that must attack a planeswalker.
        let asked = loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Decide(d) if d.kind == DecisionKind::Priority => {
                    if g.s.turn().step() > TurnStep::DeclareAttackers {
                        break false;
                    }
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
                }
                Step::Decide(d) => break d.kind == DecisionKind::Attacker,
                Step::Over(_) => break false,
            }
        };
        if !asked {
            continue;
        }
        declared += 1;
        verify_all_paths(&g.s, &g.t);
        // Stops the game again once the attackers are declared, while they are attacking.
        g.s.set_profile(Profile {
            breakpoint: Some(Breakpoint {
                turn: g.s.turn().number(),
                step: TurnStep::DeclareAttackers,
            }),
            ..Profile::EXACT
        });

        // Plays one declaration to its end, with random picks.
        loop {
            match next(&mut g.s, &g.t, &mut g.x, &mut Replay) {
                Step::Decide(d)
                    if matches!(d.kind, DecisionKind::Attacker | DecisionKind::AttackTarget) =>
                {
                    crate::reference::check_references(&g.s, &g.t)
                        .unwrap_or_else(|v| panic!("seed {seed}: {v:?}"));
                    let Candidates::Options(ref opts) = d.candidates else {
                        panic!("expected options");
                    };
                    let pick = (next_rng(&mut rng) as usize) % opts.as_slice().len();
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(pick as u16));
                }
                Step::Decide(d) if d.kind == DecisionKind::PaymentPlan => {
                    apply(&mut g.s, &g.t, &mut g.x, &mut Replay, Choice(0));
                }
                Step::Decide(_) | Step::Over(_) => break,
            }
        }
        assert!(
            !matches!(
                *g.s.pending(),
                Pending::DeclareAttackers { .. } | Pending::PayingToAttack { .. }
            ) && g.s.turn().step() == TurnStep::DeclareAttackers,
            "seed {seed}: the declaration finished"
        );
        assert_paid_for(&g, active, seed);
    }
    assert!(
        declared >= 100,
        "only {declared} boards asked for a declaration"
    );
}

/// Every creature attacking the player was paid for: {2} a Toll each, from mana that no
/// attacker without vigilance made.
fn assert_paid_for(g: &Game, active: crate::ids::Seat, seed: u64) {
    let tolls =
        g.s.battlefield()
            .iter()
            .filter(|&&slot| g.s.definition_name(&g.t, slot) == "Toll");
    let paying = g.s.battlefield().iter().filter(|&&slot| {
        matches!(
            g.s.object(slot).attack_target(),
            Some(AttackTarget::Player(_))
        )
    });
    let owed = 2 * tolls.count() * paying.count();
    let spent = g.s.battlefield().iter().filter(|&&slot| {
        let d = g.s.derived(slot);
        d.controller == active
            && g.s.object(slot).is_tapped()
            && (d.types.contains(Types::LAND)
                || !g.s.object(slot).is_attacking()
                || d.keywords.contains(Keywords::VIGILANCE))
    });
    assert_eq!(
        spent.count(),
        owed,
        "seed {seed}: the mana tapped is the cost"
    );
}
