//! What a candidate would lead to, played out on a copy of the game: what a cast or an
//! activation would tap, so the UI can light the permanents a payment taps before the player
//! commits to it and tell payment plans apart by what they tap rather than by number; and which
//! attackers a blocker may block, so the UI can take a whole block declaration in any order
//! before it sends the engine's picks; and what a count of targets lets the first target be, so
//! the UI can take the count with the target.

use beryl_engine::card::AbilityKind;
use beryl_engine::layout::Queue;
use beryl_engine::state::{ObjectFlags, Pending};
use beryl_engine::{
    Ask, Candidates, Choice, Decision, DecisionKind, GameTable, Layout, Opt, Profile, Replay,
    Scratch, Seat, SlotId, State, Step, Stops, Zone, apply, cost_being_cast, next,
};

use crate::Status;
use crate::protocol::{Payment, PlayerView, Side, TargetRef, mana_text};

/// Adds to `view`, at the open decision `d` in `s`, the ways each candidate is paid
/// ([`payments`]) and the attackers each blocker may block ([`blocks`]). They read the whole
/// state, not the viewer's observation, but only the costs, the viewer's own permanents and
/// the attackers' public characteristics decide them.
pub fn add_previews<L: Layout>(
    view: &mut PlayerView,
    s: &State<L>,
    t: &GameTable,
    d: &Decision<L>,
) {
    if let Status::Deciding { decision } = &mut view.status {
        if d.kind == DecisionKind::Waterbend {
            decision.waterbend = waterbend(s, t, d, None).map(Box::new);
        }
        for (((c, p), b), targets) in decision
            .candidates
            .iter_mut()
            .zip(payments(s, t, d))
            .zip(blocks(s, t, d))
            .zip(targets(s, t, d))
        {
            c.payments = p;
            c.blocks = b;
            c.targets = targets;
        }
    }
}

/// A copy of `s` in which every question comes back as a decision, even one with a single
/// answer, so a walk sees each pick the engine would otherwise make silently (§7.2).
fn forced<L: Layout>(s: &State<L>) -> State<L> {
    let mut copy = s.clone();
    copy.set_profile(Profile {
        stops: Stops::Forced,
        ..s.profile()
    });
    copy
}

/// The candidate at `i` as a choice.
fn choice(i: usize) -> Choice {
    Choice(u16::try_from(i).expect("at most 64 candidates"))
}

/// The attackers each candidate of `d`, the open decision in `s`, may block, by slot, in
/// candidate order: none but for a blocker of the block declaration.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the chosen
/// creatures, the defending player chooses one creature for it to block that's attacking that
/// player, a planeswalker they control, or a battle they protect." The engine asks it as the
/// pick after the blocker, with evasion pruning the attackers (architecture §11); identical
/// attackers are one candidate (§7.2).
fn blocks<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Vec<Vec<u16>> {
    let Candidates::Options(opts) = &d.candidates else {
        return vec![Vec::new(); d.len()];
    };
    opts.iter()
        .enumerate()
        .map(|(i, &o)| {
            if d.kind != DecisionKind::Blocker || !matches!(o, Opt::Block(_)) {
                return Vec::new();
            }
            let mut copy = forced(s);
            let mut scratch = Scratch::new();
            apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
            match next(&mut copy, t, &mut scratch, &mut Replay) {
                Step::Decide(Decision {
                    kind: DecisionKind::BlockTarget,
                    candidates: Candidates::Objects(attackers),
                    ..
                }) => attackers.iter().map(SlotId::num).collect(),
                _ => Vec::new(),
            }
        })
        .collect()
}

/// What the first target each candidate of `d`, the open decision in `s`, announces may be,
/// in candidate order: none but for a count of targets above zero.
///
/// [CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c): "If the spell has a
/// variable number of targets, the player announces how many targets they will choose before
/// they announce those targets." The engine asks the targets as the picks after the count;
/// identical objects are one candidate (§7.2).
fn targets<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Vec<Vec<TargetRef>> {
    let Candidates::Options(opts) = &d.candidates else {
        return vec![Vec::new(); d.len()];
    };
    let side = |p: Seat| {
        if p == d.player {
            Side::You
        } else {
            Side::Opponent
        }
    };
    opts.iter()
        .enumerate()
        .map(|(i, &o)| {
            if d.kind != DecisionKind::TargetCount || o == Opt::Number(0) {
                return Vec::new();
            }
            let mut copy = forced(s);
            let mut scratch = Scratch::new();
            apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
            match next(&mut copy, t, &mut scratch, &mut Replay) {
                Step::Decide(Decision {
                    kind: DecisionKind::Target,
                    candidates,
                    ..
                }) => match candidates {
                    Candidates::Objects(objects) => objects
                        .iter()
                        .map(|slot| TargetRef::Object { slot: slot.num() })
                        .collect(),
                    Candidates::Options(opts) => opts
                        .iter()
                        .filter_map(|o| match *o {
                            Opt::Player(p) => Some(TargetRef::Player { player: side(p) }),
                            _ => None,
                        })
                        .collect(),
                },
                _ => Vec::new(),
            }
        })
        .collect()
}

/// The ways each candidate of `d`, the open decision in `s`, can be paid, in candidate order:
/// none for a candidate that starts no cast or activation.
///
/// [CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b): "If the spell has a
/// variable cost that will be paid as it's being cast (such as an {X} in its mana cost; see
/// rule 107.3), the player announces the value of that variable." So a cast that asks for X
/// has a payment per value. [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g):
/// "If the total cost includes a mana payment, the player then has a chance to activate mana
/// abilities"; the engine settles which sources among plans with the same consequences
/// (architecture §10), so a payment per plan it offers.
fn payments<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Vec<Vec<Payment>> {
    let Candidates::Options(opts) = &d.candidates else {
        return vec![Vec::new(); d.len()];
    };
    opts.iter()
        .enumerate()
        .map(|(i, &o)| {
            let at = match (d.kind, o) {
                (_, Opt::Cast { .. } | Opt::Activate { .. } | Opt::TurnFaceUp(_)) => {
                    Payment::default()
                }
                (DecisionKind::X, Opt::Number(n)) => Payment {
                    x: Some(n),
                    ..Payment::default()
                },
                (DecisionKind::PaymentPlan, Opt::Plan(plan)) => Payment {
                    plan: Some(plan.into()),
                    ..Payment::default()
                },
                (DecisionKind::YesNo, Opt::Yes) if matches!(d.ask, Ask::UnlessPays { .. }) => {
                    return vec![unless_paid(s, t, d, i)];
                }
                _ => return Vec::new(),
            };
            // The walk sees where the cast ends.
            let at = priced(at, s, t);
            let mut copy = forced(s);
            let mut scratch = Scratch::new();
            apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
            let mut out = Vec::new();
            walk(s, copy, t, &mut scratch, &at, &mut out);
            out
        })
        .collect()
}

/// What paying an "unless pays" cost taps, by the yes at `i` of `d`, the open decision in `s`,
/// and the engine's default payment plan
/// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a): "This means the same
/// thing as '[A player may do something else]. If [that player doesn't], [do something].'"),
/// so the UI can light the permanents that would pay.
fn unless_paid<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>, i: usize) -> Payment {
    let mut copy = forced(s);
    let mut scratch = Scratch::new();
    apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
    while let Step::Decide(paying) = next(&mut copy, t, &mut scratch, &mut Replay)
        && paying.player == d.player
        && matches!(
            paying.kind,
            DecisionKind::PaymentPlan | DecisionKind::ManaPayment
        )
    {
        apply(&mut copy, t, &mut scratch, &mut Replay, Choice(0));
    }
    Payment {
        taps: tapped(s, &copy, t),
        ..Payment::default()
    }
}

/// Answers the rest of the cast in `s`: every value of X and every payment plan, each on a
/// copy, and the first answer to anything else (targets cost the same whichever is chosen).
/// Pushes what each finished cast tapped, from `before` on.
fn walk<L: Layout>(
    before: &State<L>,
    mut s: State<L>,
    t: &GameTable,
    scratch: &mut Scratch<L>,
    at: &Payment,
    out: &mut Vec<Payment>,
) {
    while matches!(
        s.pending(),
        Pending::Casting(_) | Pending::TurningFaceUp { .. }
    ) {
        let Step::Decide(d) = next(&mut s, t, scratch, &mut Replay) else {
            return;
        };
        let Candidates::Options(opts) = &d.candidates else {
            apply(&mut s, t, scratch, &mut Replay, Choice(0));
            continue;
        };
        let branch = |o: Opt<L>| match (d.kind, o) {
            (DecisionKind::X, Opt::Number(n)) => Some(Payment {
                x: Some(n),
                ..at.clone()
            }),
            (DecisionKind::PaymentPlan, Opt::Plan(plan)) if opts.len() > 1 => Some(Payment {
                plan: Some(plan.into()),
                ..at.clone()
            }),
            _ => None,
        };
        if opts
            .as_slice()
            .first()
            .is_some_and(|&o| branch(o).is_some())
        {
            for (i, &o) in opts.iter().enumerate() {
                let at = priced(branch(o).expect("one kind"), &s, t);
                let mut copy = s.clone();
                apply(&mut copy, t, scratch, &mut Replay, choice(i));
                walk(before, copy, t, scratch, &at, out);
            }
            return;
        }
        apply(&mut s, t, scratch, &mut Replay, Choice(0));
    }
    out.push(Payment {
        taps: tapped(before, &s, t),
        ..at.clone()
    });
}

/// `at` with the total cost its X makes, announced at the X decision open in `s`
/// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "The total cost is the
/// mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and
/// cost increases, and minus all cost reductions").
fn priced<L: Layout>(at: Payment, s: &State<L>, t: &GameTable) -> Payment {
    match at.x {
        Some(x) if at.cost.is_none() => Payment {
            cost: cost_being_cast(s, t, x).map(mana_text),
            ..at
        },
        _ => at,
    }
}

/// A proposed waterbend payment, computed on a copy. The supplied benders replace generic
/// mana one at a time; the remainder uses the engine's usual mana plan. No preview changes
/// the real state. [CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a):
/// "you may tap an untapped artifact or creature you control rather than pay that mana."
pub(crate) fn waterbend<L: Layout>(
    before: &State<L>,
    t: &GameTable,
    first: &Decision<L>,
    selected: Option<&[u16]>,
) -> Option<crate::ipc::WaterbendPreview> {
    if selected.is_some_and(|slots| {
        slots
            .iter()
            .enumerate()
            .any(|(i, slot)| slots[..i].contains(slot))
    }) {
        return None;
    }
    let mut copy = forced(before);
    let mut scratch = Scratch::new();
    let mut decision = first.clone();
    let selected = selected.unwrap_or_default();
    let mut lands = Vec::new();
    let mut benders = Vec::new();
    let mut plan = None;
    loop {
        let index = match decision.kind {
            DecisionKind::Waterbend => {
                let desired = selected.get(lands.len() + benders.len());
                let wanted = desired.and_then(|&wanted| {
                    before
                        .battlefield()
                        .iter()
                        .copied()
                        .find(|s| s.num() == wanted)
                });
                // A land that can also waterbend uses its mana ability when clicked.
                (0..decision.len()).rev().find(|&i| {
                    match decision.candidates.option(choice(i)) {
                        Some(Opt::Object(slot) | Opt::Mana { slot, .. }) => {
                            wanted.is_some_and(|wanted| {
                                beryl_engine::interchangeable(before, slot, wanted)
                            })
                        }
                        Some(Opt::Done) => desired.is_none(),
                        _ => false,
                    }
                })?
            }
            DecisionKind::PaymentPlan => 0,
            _ => return None,
        };
        match decision.candidates.option(choice(index)) {
            Some(Opt::Mana { slot, color }) => lands.push(crate::ipc::WaterbendLand {
                slot: slot.num(),
                color: color.into(),
            }),
            Some(Opt::Object(slot)) => benders.push(slot.num()),
            Some(Opt::Plan(p)) => plan = Some(p.into()),
            _ => {}
        }
        apply(&mut copy, t, &mut scratch, &mut Replay, choice(index));
        let step = next(&mut copy, t, &mut scratch, &mut Replay);
        if !matches!(copy.pending(), Pending::Casting(_)) {
            break;
        }
        let Step::Decide(next) = step else {
            return None;
        };
        decision = next;
    }
    if selected.len() != lands.len() + benders.len() {
        return None;
    }
    Some(crate::ipc::WaterbendPreview {
        lands,
        benders,
        taps: tapped(before, &copy, t),
        plan,
    })
}

/// The permanents untapped in `before` and tapped in `after`, by slot, with those the payment
/// sacrificed for mana, as a Treasure
/// ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)): "{T}, Sacrifice this
/// token: Add one mana of any color." A permanent sacrificed otherwise, as Strip Mine for its
/// own ability, paid no mana.
fn tapped<L: Layout>(before: &State<L>, after: &State<L>, t: &GameTable) -> Vec<u16> {
    let untapped = |s: &State<L>, slot| {
        let o = s.object(slot);
        o.zone == Zone::Battlefield && !o.flags.has(ObjectFlags::TAPPED)
    };
    let sacrifices_for_mana = |slot: L::Slot| {
        let face = t.def(before.card(slot)).face(before.object(slot).face);
        face.abilities
            .iter()
            .any(|a| a.kind == AbilityKind::Mana && a.cost.sacrifice_self)
    };
    before
        .battlefield()
        .iter()
        .filter(|&&slot| untapped(before, slot))
        .filter(|&&slot| {
            if after.is_current(before.object_ref(slot)) {
                !untapped(after, slot)
            } else {
                sacrifices_for_mana(slot)
            }
        })
        .map(|slot| slot.num())
        .collect()
}

#[cfg(test)]
mod tests {
    use beryl_cards::{CARDS, names};
    use beryl_engine::card::{AbilityIx, Castings};
    use beryl_engine::{Slot, TurnStep};
    use beryl_scenario::{P0, P1, Permanent, Scenario};

    use super::*;

    #[test]
    fn waterbend_preview_uses_identical_benders_and_rejects_unpayable_edits() {
        const SCENARIO: Scenario = Scenario::new("waterbend previews").battlefield(
            P0,
            &[
                Permanent::new(names::AANGS_ICEBERG),
                Permanent::new(names::PLAINS),
                Permanent::new(names::SNAPCASTER_MAGE),
                Permanent::new(names::SNAPCASTER_MAGE),
            ],
        );
        let (t, mut s) = beryl_scenario::position(&CARDS, &SCENARIO).unwrap();
        s.set_profile(Profile {
            taps_mana: Some(P0),
            ..Profile::SMART
        });
        let mut scratch = Scratch::new();
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("priority");
        };
        let activate = (0..d.len()).find(|&i| matches!(d.candidates.option(choice(i)), Some(Opt::Activate { slot, .. }) if t.def(s.card(slot)).name == names::AANGS_ICEBERG)).unwrap();
        apply(&mut s, &t, &mut scratch, &mut Replay, choice(activate));
        let Step::Decide(d) = next(&mut s, &t, &mut scratch, &mut Replay) else {
            panic!("waterbend");
        };
        assert_eq!(d.kind, DecisionKind::Waterbend);
        let benders: Vec<_> = s
            .battlefield()
            .iter()
            .filter(|&&slot| t.def(s.card(slot)).name == names::SNAPCASTER_MAGE)
            .map(|slot| slot.num())
            .collect();
        assert!(
            waterbend(&s, &t, &d, None).is_none(),
            "no bender is implied"
        );
        let land = d.candidates.option(choice(d.len() - 1)).unwrap();
        let Opt::Mana { slot, .. } = land else {
            panic!("land is an explicit payment choice");
        };
        let mut selected = vec![slot.num()];
        selected.extend(benders.iter().rev().copied());
        let mixed = waterbend(&s, &t, &d, Some(&selected)).unwrap();
        assert_eq!(mixed.lands[0].slot, slot.num());
        assert_eq!(mixed.benders, benders);
        assert_eq!(mixed.taps.len(), 3);
        assert!(waterbend(&s, &t, &d, Some(&benders[..1])).is_none());
        assert!(waterbend(&s, &t, &d, Some(&[benders[0], benders[0]])).is_none());
        assert!(
            s.battlefield()
                .iter()
                .all(|&slot| !s.object(slot).flags.has(ObjectFlags::TAPPED))
        );
    }

    /// The first decision of a scenario with `hand` and `battlefield` for the first player,
    /// in their main phase, and the ways each of its candidates pays.
    fn priority(hand: &'static [&'static str], battlefield: &'static [Permanent]) -> Paid {
        let scenario = Scenario::new("payments")
            .hand(P0, hand)
            .battlefield(P0, battlefield);
        let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
        let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
            panic!("a decision");
        };
        assert_eq!(d.kind, DecisionKind::Priority);
        let payments = payments(&s, &t, &d);
        let Candidates::Options(opts) = d.candidates else {
            panic!("options");
        };
        let named = |slot: Slot| t.def(s.card(slot)).name;
        Paid(
            opts.iter()
                .zip(payments)
                .map(|(&o, p)| {
                    let card = match o {
                        Opt::Cast { slot, costs, .. } => {
                            Some((named(slot), costs == Castings::NONE))
                        }
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
                                    .map(|&t| named(Slot::from_index(usize::from(t))))
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
            .battlefield(P0, ISLANDS);
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
            .battlefield(P0, &[Permanent::new(names::ISLAND); 4])
            .hand(P0, &[names::THUNDERTRAP_TRAINER])
            .library(P0, &[names::ISLAND; 4])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND; 4])
            .actions(&[
                Action::cast(P0, names::THUNDERTRAP_TRAINER),
                Action::cast(P1, names::MANA_TITHE),
                Action::answer(P0, DecisionKind::YesNo, Pick::No),
            ]);
        let mut seen = None;
        beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
            if matches!(d.ask, Ask::UnlessPays { .. }) {
                let untapped = |slot: u16| {
                    !s.object(Slot::from_index(usize::from(slot)))
                        .flags
                        .has(ObjectFlags::TAPPED)
                };
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
            .battlefield(P0, &[Permanent::new(names::PLAINS); 5])
            .library(P0, &[names::ISLAND])
            .battlefield(P1, SIX_PLAINS)
            .actions(&[
                Action::cast(P0, names::BEZA_THE_BOUNDING_SPRING),
                Action::Resolve,
                Action::Resolve,
                Action::cast(P0, names::THIEVING_SKYDIVER),
            ]);
        let mut taps = Vec::new();
        beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
            let named = |slot: u16| t.def(s.card(Slot::from_index(usize::from(slot)))).name;
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
            .battlefield(P0, ATTACKERS)
            .battlefield(P1, BLOCKERS);
        let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
        let named = |s: &State, slot: Slot| t.def(s.card(slot)).name;
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
                    .map(|a| named(&s, Slot::from_index(usize::from(a))));
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
            .battlefield(P0, TEFERI)
            .battlefield(P1, STAFF);
        let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
        let named = |s: &State, slot: u16| t.def(s.card(Slot::from_index(usize::from(slot)))).name;
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
                    |o| matches!(o, Opt::Activate { ability, .. } if *ability == AbilityIx::Own(2)),
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
}
