//! Mana abilities other objects grant a permanent, as Gemhide Sliver's "All Slivers have
//! '{T}: Add one mana of any color.'" does ([CR 613.1f]): "Layer 6: Ability-adding effects
//! ... are applied." The mana solver reads them from the abilities the permanent has, and a
//! creature granted one still can't tap for it until it has been under its controller's
//! control since their most recent turn began ([CR 302.6]).
//!
//! [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
//! [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026

use beryl_card::{ManaSet, Objects, Subtype, Subtypes};
use beryl_kernel::two_player::TwoPlayerSeat as _;

use super::LAND;
use super::cast::{Game, ISLAND};
use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Face, LayerSpec, ManaColor, ManaCost, Modification, Types};
use crate::filter::Filter;
use crate::mana;
use crate::replacement::event::Sink;
use crate::rng::SplitMix64;
use crate::state::{ManaPool, Zone};
use crate::{CardTable, Decklist, Opt, Profile};

/// "{T}: Add one mana of any color."
static ANY_COLOR: [Ability; 1] = [Ability::tap_for_mana(ManaSet::any_color(1))];

/// A 1/1 Sliver: "All Slivers have '{T}: Add one mana of any color.'"
static GEM: CardDef = CardDef {
    name: "Test Gem Sliver",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Sliver])),
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
        grants: &ANY_COLOR,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A vanilla 1/1 Sliver.
static SLIVER: CardDef = CardDef {
    name: "Test Sliver",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Sliver])),
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// An artifact with no mana ability of its own: "Artifacts and lands have '{T}: Add one mana
/// of any color.'" It is granted the ability itself, a noncreature permanent whose card's
/// facts say it makes no mana.
static LANTERN: CardDef = CardDef {
    name: "Test Lantern",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::ARTIFACT.union(Types::LAND)),
            },
            condition: None,
            modifications: &[Modification::grant(0)],
        })],
        grants: &ANY_COLOR,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &ISLAND, &GEM, &SLIVER, &LANTERN]);
const DECK: &Decklist = &[
    (8, "Test Land"),
    (8, "Test Island"),
    (6, "Test Gem Sliver"),
    (8, "Test Sliver"),
    (4, "Test Lantern"),
];

/// Two Gem Slivers give each Sliver two instances of the ability (see CR 113.2c), but
/// activating one is activating the same ability as the other: a player tapping mana at
/// priority is offered each color once for the Gem Slivers, and an opponent's Gem Sliver
/// grants the player's Sliver the ability too, which the player activates.
#[test]
fn two_instances_of_a_granted_mana_ability_are_one_option_for_each_color() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    g.s.set_profile(Profile {
        taps_mana: Some(g.me),
        ..Profile::EXACT
    });
    let gems = [
        g.give(g.me, "Test Gem Sliver", Zone::Battlefield),
        g.give(g.me, "Test Gem Sliver", Zone::Battlefield),
    ];
    g.give(g.them, "Test Gem Sliver", Zone::Battlefield);
    let sliver = g.give(g.me, "Test Sliver", Zone::Battlefield);
    // A land to play, so that the player has a priority decision.
    g.give(g.me, "Test Island", Zone::Hand);
    let d = g.decide();
    assert_eq!(d.kind, crate::DecisionKind::Priority);
    let options: Vec<Opt> = (0..d.len())
        .filter_map(|i| d.candidates.option(crate::Choice(i as u16)))
        .collect();
    for color in [
        ManaColor::White,
        ManaColor::Blue,
        ManaColor::Black,
        ManaColor::Red,
        ManaColor::Green,
    ] {
        let of_gems = |&o: &&Opt| matches!(o, Opt::Mana { slot, color: c } if gems.contains(slot) && *c == color);
        assert_eq!(
            options.iter().filter(of_gems).count(),
            1,
            "{color:?}: {options:?}"
        );
        assert!(
            options.contains(&Opt::Mana {
                slot: sliver,
                color
            }),
            "{options:?}"
        );
    }
    assert!(!options.iter().any(|o| matches!(
        o,
        Opt::Mana {
            color: ManaColor::Colorless,
            ..
        }
    )));
    g.pick(Opt::Mana {
        slot: sliver,
        color: ManaColor::Red,
    });
    assert!(g.s.object(sliver).is_tapped());
    assert_eq!(g.s.player(g.me).mana().amounts(), [0, 0, 0, 1, 0, 0]);
}

/// On 200 seeded boards of lands, Slivers, Gem Slivers and Lanterns on both sides, some
/// creatures summoning sick, the cached funds, the payability prefilter, the solver's plans
/// (paid out, each leaving the pool it says) and every manual payment agree with the
/// reference of payment sequences, which reads each permanent's abilities.
#[test]
#[allow(clippy::too_many_lines)]
fn the_payment_reference_agrees_on_boards_with_granted_mana_abilities() {
    let t = crate::GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [DECK, DECK],
    )
    .unwrap();
    let mut rng = SplitMix64::new(0x6E3_5117);
    let (mut payable, mut unpayable, mut manual_steps) = (0usize, 0usize, 0usize);
    for board_ix in 0..200u64 {
        let mut s = crate::State::new(&t, board_ix + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let mut placed = Vec::new();
        let counts = [
            (p, "Test Gem Sliver", rng.below(3)),
            (p, "Test Lantern", rng.below(2)),
            (p, "Test Sliver", rng.below(4)),
            (p, "Test Land", rng.below(3)),
            (p, "Test Island", rng.below(3)),
            (p.opponent(), "Test Gem Sliver", rng.below(2)),
            (p.opponent(), "Test Lantern", rng.below(2)),
        ];
        for (owner, name, n) in counts {
            for _ in 0..n {
                let Some(&slot) =
                    (s.library(owner).iter()).find(|&&slot| s.definition_name(&t, slot) == name)
                else {
                    continue;
                };
                s.board().place(slot, Zone::Battlefield).unwrap();
                // Some creatures came under their controller's control this turn.
                let sick = name.contains("Sliver") && rng.below(3) == 0;
                s.board().set_sick(slot, sick);
                placed.push((owner == p, name, sick));
            }
        }
        let mut pool = ManaPool::default();
        pool[ManaColor::Colorless] = rng.below(2) as u8;
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        crate::reference::check_references(&s, &t).unwrap();
        let cost = ManaCost::new(
            rng.below(3) as u8,
            [
                rng.below(2) as u8,
                rng.below(3) as u8,
                0,
                rng.below(2) as u8,
                rng.below(2) as u8,
                0,
            ],
        );
        let expected = crate::reference::reference_sequences(&s, &t, p, cost)
            .expect("the reference reads every ability of these boards");
        let ctx = format!("board {board_ix}: cost {cost:?}, pool {pool:?}, placed {placed:?}");
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
                "mana::plans(greedy={greedy}): {ctx}"
            );
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
                    "plan.left: {ctx}"
                );
            }
        }
        if !expected {
            unpayable += 1;
            continue;
        }
        payable += 1;
        // Under manual payment, every sequence of offered options ends with the cost paid.
        for trial in 0..3 {
            let (mut ms, mut remaining) = (s, cost);
            for _ in 0..40 {
                if remaining.is_zero() {
                    break;
                }
                let options = mana::manual_options(
                    &ms,
                    &t,
                    p,
                    remaining,
                    Bitboard::EMPTY,
                    mana::Paying::NOTHING,
                )
                .unwrap();
                assert!(!options.is_empty(), "no option for {remaining:?}: {ctx}");
                let pick = match trial {
                    0 => 0,
                    1 => options.len() - 1,
                    _ => rng.below(options.len() as u64) as usize,
                };
                let opt = options.as_slice()[pick];
                let paying = mana::Paying::NOTHING;
                remaining =
                    mana::manual_pay(&mut ms, &t, &mut Sink::off(), p, remaining, opt, paying)
                        .unwrap()
                        .0;
                manual_steps += 1;
            }
            assert!(remaining.is_zero(), "manual payment did not finish: {ctx}");
        }
    }
    assert!(
        payable > 50 && unpayable > 20,
        "{payable} payable, {unpayable} not"
    );
    assert!(manual_steps > 100, "only {manual_steps} manual steps");
}
