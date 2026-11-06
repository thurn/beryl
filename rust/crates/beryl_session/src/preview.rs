//! What a candidate would lead to, played out on a copy of the game: what a cast or an
//! activation would tap, so the UI can light the permanents a payment taps before the player
//! commits to it and tell payment plans apart by what they tap rather than by number; and which
//! attackers a blocker may block, so the UI can take a whole block declaration in any order
//! before it sends the engine's picks.

use beryl_engine::state::{ObjectFlags, Pending};
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Opt, Profile, Replay, Scratch, Slot,
    State, Step, Stops, Zone, apply, next,
};

use crate::Status;
use crate::protocol::{Payment, PlayerView};

/// Adds to `view`, at the open decision `d` in `s`, the ways each candidate is paid
/// ([`payments`]) and the attackers each blocker may block ([`blocks`]). They read the whole
/// state, not the viewer's observation, but only the costs, the viewer's own permanents and
/// the attackers' public characteristics decide them.
pub fn add_previews(view: &mut PlayerView, s: &State, t: &GameTable, d: &Decision) {
    if let Status::Deciding { decision } = &mut view.status {
        for ((c, p), b) in decision
            .candidates
            .iter_mut()
            .zip(payments(s, t, d))
            .zip(blocks(s, t, d))
        {
            c.payments = p;
            c.blocks = b;
        }
    }
}

/// A copy of `s` in which every question comes back as a decision, even one with a single
/// answer, so a walk sees each pick the engine would otherwise make silently (§7.2).
fn forced(s: &State) -> State {
    let mut copy = *s;
    copy.set_profile(Profile {
        stops: Stops::Forced,
        ..s.profile()
    });
    copy
}

/// The candidate at `i` as a choice.
fn choice(i: usize) -> Choice {
    Choice(u8::try_from(i).expect("at most 64 candidates"))
}

/// The attackers each candidate of `d`, the open decision in `s`, may block, by slot, in
/// candidate order: none but for a blocker of the block declaration.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the chosen
/// creatures, the defending player chooses one creature for it to block that's attacking that
/// player, a planeswalker they control, or a battle they protect." The engine asks it as the
/// pick after the blocker, with evasion pruning the attackers (architecture §11); identical
/// attackers are one candidate (§7.2).
fn blocks(s: &State, t: &GameTable, d: &Decision) -> Vec<Vec<u8>> {
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
                }) => attackers.iter().map(Slot::get).collect(),
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
fn payments(s: &State, t: &GameTable, d: &Decision) -> Vec<Vec<Payment>> {
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
                    plan: Some(plan),
                    ..Payment::default()
                },
                _ => return Vec::new(),
            };
            // The walk sees where the cast ends.
            let mut copy = forced(s);
            let mut scratch = Scratch::new();
            apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
            let mut out = Vec::new();
            walk(s, copy, t, &mut scratch, &at, &mut out);
            out
        })
        .collect()
}

/// Answers the rest of the cast in `s`: every value of X and every payment plan, each on a
/// copy, and the first answer to anything else (targets cost the same whichever is chosen).
/// Pushes what each finished cast tapped, from `before` on.
fn walk(
    before: &State,
    mut s: State,
    t: &GameTable,
    scratch: &mut Scratch,
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
        let branch = |o: Opt| match (d.kind, o) {
            (DecisionKind::X, Opt::Number(n)) => Some(Payment {
                x: Some(n),
                ..at.clone()
            }),
            (DecisionKind::PaymentPlan, Opt::Plan(plan)) if opts.len() > 1 => Some(Payment {
                plan: Some(plan),
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
                let mut copy = s;
                apply(&mut copy, t, scratch, &mut Replay, choice(i));
                walk(before, copy, t, scratch, &branch(o).expect("one kind"), out);
            }
            return;
        }
        apply(&mut s, t, scratch, &mut Replay, Choice(0));
    }
    out.push(Payment {
        taps: tapped(before, &s),
        ..at.clone()
    });
}

/// The permanents untapped in `before` and tapped in `after`, by slot.
fn tapped(before: &State, after: &State) -> Vec<u8> {
    let untapped = |s: &State, slot| {
        let o = s.object(slot);
        o.zone == Zone::Battlefield && !o.flags.has(ObjectFlags::TAPPED)
    };
    before
        .battlefield()
        .iter()
        .filter(|&&slot| untapped(before, slot) && after.object(slot).zone == Zone::Battlefield)
        .filter(|&&slot| !untapped(after, slot))
        .map(|slot| slot.get())
        .collect()
}

#[cfg(test)]
mod tests {
    use beryl_cards::{CARDS, names};
    use beryl_engine::TurnStep;
    use beryl_engine::card::Castings;
    use beryl_scenario::{P0, P1, Permanent, Scenario};

    use super::*;

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
                                p.taps.iter().map(|&t| named(Slot::new(t))).collect(),
                            )
                        })
                        .collect();
                    (card, taps)
                })
                .collect(),
        )
    }

    /// A payment as (X, plan, the names of the permanents it taps).
    type Named = (Option<u8>, Option<u8>, Vec<&'static str>);

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
        // left still cast Mana Drain ({U}{U}). With Get Lost ({1}{W}) in hand too, either
        // payment keeps one card castable; it keeps the one hungrier for colors.
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
        ] {
            let paid = priority(hand.to_vec().leak(), LANDS);
            assert_eq!(
                paid.cast(jace, true),
                [(None, None, vec![names::ISLAND, names::PLAINS])],
                "with {hand:?}"
            );
        }
        // Two cards that want {W} outnumber the one that wants {U}{U}.
        let hand = [jace, names::MANA_DRAIN, names::GET_LOST, names::OUST];
        let paid = priority(hand.to_vec().leak(), LANDS);
        assert_eq!(
            paid.cast(jace, true),
            [(None, None, vec![names::ISLAND, names::ISLAND])]
        );
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
                let attackers = b.into_iter().map(|a| named(&s, Slot::new(a)));
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
}
