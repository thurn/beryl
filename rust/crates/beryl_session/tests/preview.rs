//! What a candidate would lead to ([`add_previews`]): the permanents a payment taps, the
//! attackers a blocker may block and what a count of targets lets the first target be, read
//! from the view a player is sent.

use beryl_cards::{CARDS, names};
use beryl_engine::card::Purpose;
use beryl_engine::card::{Castings, ObjectAbility};
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Replay, Scratch, Slot,
    SlotId as _, State, Step, TurnStep, apply, next,
};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_session::preview::add_previews;
use beryl_session::protocol::{DecisionId, Moment, player_view};
use beryl_session::protocol::{ObjectId, Payment, TargetRef};
use beryl_session::{SessionId, Status, StopsSetting};

/// The candidate at `i` as a choice.
fn choice(i: usize) -> Choice {
    Choice(u16::try_from(i).expect("at most 64 candidates"))
}

/// What the player deciding `d` in `s` is shown for each of its candidates, in candidate order:
/// the ways it is paid, the attackers it may block and the targets it may pick first.
fn previewed(
    s: &State,
    t: &GameTable,
    d: &Decision,
) -> Vec<(Vec<Payment>, Vec<ObjectId>, Vec<TargetRef>)> {
    let id = DecisionId {
        session: SessionId(1),
        seq: 1,
    };
    let mut view = player_view(
        t,
        Observation::new(s, d.decider),
        Moment::Deciding(id, d, None),
        StopsSetting::SMART,
        &[],
    );
    add_previews(&mut view, s, t, d);
    let Status::Deciding { decision } = view.status else {
        panic!("the decision is open");
    };
    decision
        .candidates
        .into_iter()
        .map(|c| (c.payments, c.blocks, c.targets))
        .collect()
}

fn payments(s: &State, t: &GameTable, d: &Decision) -> Vec<Vec<Payment>> {
    previewed(s, t, d).into_iter().map(|c| c.0).collect()
}

fn blocks(s: &State, t: &GameTable, d: &Decision) -> Vec<Vec<ObjectId>> {
    previewed(s, t, d).into_iter().map(|c| c.1).collect()
}

fn targets(s: &State, t: &GameTable, d: &Decision) -> Vec<Vec<TargetRef>> {
    previewed(s, t, d).into_iter().map(|c| c.2).collect()
}

/// The first decision of a scenario with `hand` and `battlefield` for the first player,
/// in their main phase, and the ways each of its candidates pays.
fn priority(hand: &'static [&'static str], battlefield: &'static [Permanent]) -> Paid {
    let scenario = Scenario::new("payments")
        .hand(P0, hand)
        .permanents(P0, battlefield);
    let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("a decision");
    };
    assert_eq!(d.kind, DecisionKind::Priority);
    let payments = payments(&s, &t, &d);
    let Candidates::Options(opts) = d.candidates else {
        panic!("options");
    };
    let named = |slot: Slot| s.definition_name(&t, slot);
    Paid(
        opts.iter()
            .zip(payments)
            .map(|(&o, p)| {
                let card = match o {
                    Opt::Cast { slot, costs, .. } => Some((named(slot), costs == Castings::NONE)),
                    _ => None,
                };
                let taps = p
                    .into_iter()
                    .map(|p| {
                        (
                            p.x,
                            p.plan,
                            p.taps
                                .iter()
                                .map(|&t| named(Slot::from_num(t.0).unwrap()))
                                .collect(),
                        )
                    })
                    .collect();
                (card, taps)
            })
            .collect(),
    )
}

/// A payment as (X, plan, the names of the permanents it taps).
type Named = (Option<u8>, Option<u16>, Vec<&'static str>);

/// A cast as (the card, whether with no optional cost).
type Cast = Option<(&'static str, bool)>;

/// Per candidate: the cast it is, if one, and its payments.
struct Paid(Vec<(Cast, Vec<Named>)>);

impl Paid {
    fn cast(&self, card: &str, plain: bool) -> &[Named] {
        &self
            .0
            .iter()
            .find(|(c, _)| *c == Some((card, plain)))
            .expect("the cast is offered")
            .1
    }
}

const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 4];

#[test]
fn a_kicked_cast_pays_for_each_value_of_x() {
    let paid = priority(&[names::THIEVING_SKYDIVER], ISLANDS);
    let island = names::ISLAND;
    // {1}{U}: two Islands.
    assert_eq!(
        paid.cast(names::THIEVING_SKYDIVER, true),
        [(None, None, vec![island; 2])]
    );
    // Kicker {X}, X from 1 as far as four Islands go.
    assert_eq!(
        paid.cast(names::THIEVING_SKYDIVER, false),
        [
            (Some(1), None, vec![island; 3]),
            (Some(2), None, vec![island; 4])
        ]
    );
    // Nothing to pay for anything else: passing.
    assert!(
        paid.0
            .iter()
            .filter(|(c, _)| c.is_none())
            .all(|(_, p)| p.is_empty())
    );
}

#[test]
fn each_value_of_x_says_the_total_cost_it_makes() {
    let scenario = Scenario::new("payments")
        .hand(P0, &[names::THIEVING_SKYDIVER])
        .permanents(P0, ISLANDS);
    let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("a decision");
    };
    let costs: Vec<_> = payments(&s, &t, &d)
        .into_iter()
        .flatten()
        .map(|p| (p.x, p.cost))
        .collect();
    // {1}{U}, kicker {X}: the plain cast has no X to price.
    assert_eq!(
        costs,
        [
            (None, None),
            (Some(1), Some("{2}{U}".to_owned())),
            (Some(2), Some("{3}{U}".to_owned())),
        ]
    );
}

#[test]
fn paying_for_a_spell_unless_countered_lights_what_pays() {
    use beryl_scenario::{Action, Pick};
    // Thundertrap Trainer taps two of four lands; Mana Tithe asks {1} of the rest.
    const SCENARIO: Scenario = Scenario::new("payments")
        .at(3, P0, TurnStep::PrecombatMain)
        .permanents(P0, &[Permanent::new(names::ISLAND); 4])
        .hand(P0, &[names::THUNDERTRAP_TRAINER])
        .library(P0, &[names::ISLAND; 4])
        .permanents(P1, &[Permanent::new(names::PLAINS)])
        .hand(P1, &[names::MANA_TITHE])
        .library(P1, &[names::ISLAND; 4])
        .actions(&[
            Action::cast(P0, names::THUNDERTRAP_TRAINER),
            Action::cast(P1, names::MANA_TITHE),
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
        ]);
    let mut seen = None;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        if d.prompt.purpose == Purpose::UnlessPays {
            let untapped =
                |ObjectId(slot): ObjectId| !s.object(Slot::from_num(slot).unwrap()).is_tapped();
            let paid = payments(s, t, d);
            let yes = (0..d.len())
                .position(|i| d.candidates.option(choice(i)) == Some(Opt::Yes))
                .expect("a yes");
            seen = Some(
                paid.iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let taps = p.iter().flat_map(|p| p.taps.iter().copied());
                        (i == yes, taps.filter(|&s| untapped(s)).count())
                    })
                    .collect::<Vec<_>>(),
            );
        }
    })
    .unwrap();
    // Yes taps one untapped Island; No taps nothing.
    let mut seen = seen.expect("the payment is asked");
    seen.sort_unstable();
    assert_eq!(seen, [(false, 0), (true, 1)]);
}

#[test]
fn a_payment_shows_the_treasure_it_sacrifices() {
    use beryl_cards::tokens::TREASURE_TOKEN;
    use beryl_scenario::Action;
    const SIX_PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS); 6];
    // Beza taps four of five Plains and makes a Treasure (P1 has six lands to five), then
    // the last Plains and the Treasure cast the Skydiver.
    const SCENARIO: Scenario = Scenario::new("payments")
        .hand(
            P0,
            &[names::BEZA_THE_BOUNDING_SPRING, names::THIEVING_SKYDIVER],
        )
        .permanents(P0, &[Permanent::new(names::PLAINS); 5])
        .library(P0, &[names::ISLAND])
        .permanents(P1, SIX_PLAINS)
        .actions(&[
            Action::cast(P0, names::BEZA_THE_BOUNDING_SPRING),
            Action::Resolve,
            Action::Resolve,
            Action::cast(P0, names::THIEVING_SKYDIVER),
        ]);
    let mut taps = Vec::new();
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        let named = |ObjectId(slot): ObjectId| s.definition_name(t, Slot::from_num(slot).unwrap());
        taps = payments(s, t, d)
            .into_iter()
            .zip((0..d.len()).map(|i| d.candidates.option(choice(i))))
            .filter(|(_, o)| {
                matches!(
                    o,
                    Some(Opt::Cast {
                        costs: Castings::NONE,
                        ..
                    })
                )
            })
            .flat_map(|(p, _)| p)
            .map(|p| p.taps.iter().map(|&t| named(t)).collect::<Vec<_>>())
            .collect();
    })
    .unwrap();
    assert_eq!(taps, [vec![names::PLAINS, TREASURE_TOKEN]]);
}

#[test]
fn plans_that_leave_different_mana_floating_tap_different_permanents() {
    const LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::MANA_CRYPT),
    ];
    let paid = priority(&[names::THIEVING_SKYDIVER], LANDS);
    // Two Islands, leaving nothing floating; or an Island and the Crypt, leaving {C}.
    assert_eq!(
        paid.cast(names::THIEVING_SKYDIVER, true),
        [
            (None, Some(0), vec![names::ISLAND, names::ISLAND]),
            (None, Some(1), vec![names::ISLAND, names::MANA_CRYPT]),
        ]
    );
}

#[test]
fn a_payment_keeps_open_the_colours_the_hand_needs() {
    // {1}{U} from three Islands and a Plains: the Plains pays {1}, so the two Islands
    // left still cast Mana Drain ({U}{U}), even with cards in hand that want {W}, of
    // which the two lands left could cast only one.
    const LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::PLAINS),
    ];
    let jace = names::JACE_VRYNS_PRODIGY;
    for hand in [
        &[jace, names::MANA_DRAIN][..],
        &[jace, names::MANA_DRAIN, names::GET_LOST],
        &[jace, names::GET_LOST, names::MANA_DRAIN],
        &[jace, names::MANA_DRAIN, names::GET_LOST, names::OUST],
    ] {
        let paid = priority(hand.to_vec().leak(), LANDS);
        assert_eq!(
            paid.cast(jace, true),
            [(None, None, vec![names::ISLAND, names::PLAINS])],
            "with {hand:?}"
        );
    }
}

#[test]
fn a_blocker_may_block_the_attackers_its_evasion_allows() {
    const ATTACKERS: &[Permanent] = &[
        Permanent::new(names::THUNDERTRAP_TRAINER),
        Permanent::new(names::THIEVING_SKYDIVER),
    ];
    const BLOCKERS: &[Permanent] = &[
        Permanent::new(names::ELITE_SPELLBINDER),
        Permanent::new(names::OVERLORD_OF_THE_MISTMOORS),
    ];
    let scenario = Scenario::new("blocks")
        .at(2, P0, TurnStep::BeginningOfCombat)
        .permanents(P0, ATTACKERS)
        .permanents(P1, BLOCKERS);
    let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let named = |s: &State, slot: Slot| s.definition_name(&t, slot);
    let mut scratch = Scratch::new();
    // Pass to combat and attack with both; the defending player's block declaration opens.
    let d = loop {
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("a decision");
        };
        let Candidates::Options(opts) = &d.candidates else {
            panic!("options");
        };
        let pick = match d.kind {
            DecisionKind::Blocker => break d,
            DecisionKind::Attacker => opts
                .iter()
                .position(|o| matches!(o, Opt::Attack(_)))
                .unwrap_or(0),
            _ => opts.iter().position(|o| *o == Opt::Pass).unwrap_or(0),
        };
        apply(&mut s, &t, &mut scratch, &mut Replay, choice(pick));
    };
    let Candidates::Options(opts) = &d.candidates else {
        panic!("options");
    };
    let blocks: Vec<_> = opts
        .iter()
        .zip(blocks(&s, &t, &d))
        .filter_map(|(o, b)| {
            let Opt::Block(blocker) = *o else {
                assert!(b.is_empty(), "only a blocker blocks");
                return None;
            };
            let attackers = b
                .into_iter()
                .map(|a| named(&s, Slot::from_num(a.0).unwrap()));
            Some((named(&s, blocker), attackers.collect::<Vec<_>>()))
        })
        .collect();
    // A flyer blocks either attacker; a creature without flying or reach only the one
    // without flying (CR 702.9b).
    assert_eq!(
        blocks,
        [
            (
                names::ELITE_SPELLBINDER,
                vec![names::THUNDERTRAP_TRAINER, names::THIEVING_SKYDIVER]
            ),
            (
                names::OVERLORD_OF_THE_MISTMOORS,
                vec![names::THUNDERTRAP_TRAINER]
            ),
        ]
    );
}

#[test]
fn up_to_one_target_previews_what_the_target_may_be() {
    const TEFERI: &[Permanent] = &[Permanent::new(names::TEFERI_TIME_RAVELER)];
    const STAFF: &[Permanent] = &[Permanent::new(names::STAFF_OF_THE_STORYTELLER)];
    let scenario = Scenario::new("targets")
        .permanents(P0, TEFERI)
        .permanents(P1, STAFF);
    let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
    let named =
        |s: &State, ObjectId(slot): ObjectId| s.definition_name(&t, Slot::from_num(slot).unwrap());
    let mut scratch = Scratch::new();
    // Teferi's −3, its third ability: "Return up to one target artifact, creature, or
    // enchantment to its owner's hand."
    let d = loop {
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("a decision");
        };
        if d.kind == DecisionKind::TargetCount {
            break d;
        }
        let Candidates::Options(opts) = &d.candidates else {
            panic!("options");
        };
        let minus = opts
            .iter()
            .position(
                |o| matches!(o, Opt::Activate { ability, .. } if *ability == ObjectAbility::own(2)),
            )
            .expect("Teferi's −3");
        apply(&mut s, &t, &mut scratch, &mut Replay, choice(minus));
    };
    let Candidates::Options(opts) = &d.candidates else {
        panic!("options");
    };
    let previews: Vec<_> = opts
        .iter()
        .zip(targets(&s, &t, &d))
        .map(|(&o, targets)| {
            let what: Vec<_> = targets
                .into_iter()
                .map(|target| match target {
                    TargetRef::Object { slot } => named(&s, slot),
                    TargetRef::Player { .. } => panic!("a player target"),
                    TargetRef::Ability { .. } => panic!("an ability target"),
                })
                .collect();
            (o, what)
        })
        .collect();
    // No target names nothing; one target, the Staff, the only artifact, creature or
    // enchantment.
    assert_eq!(
        previews,
        [
            (Opt::Number(0), vec![]),
            (Opt::Number(1), vec![names::STAFF_OF_THE_STORYTELLER]),
        ]
    );
}
