//! What a cast or an activation would tap: each candidate that starts or continues one is played
//! out on a copy of the game, so the UI can light the permanents a payment taps before the
//! player commits to it, and tell payment plans apart by what they tap rather than by number.

use beryl_engine::state::{ObjectFlags, Pending};
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Opt, Profile, Replay, Scratch, State,
    Step, Stops, Zone, apply, next,
};

use crate::Status;
use crate::protocol::{Payment, PlayerView};

/// Adds to `view`, at the open decision `d` in `s`, the ways each candidate is paid
/// ([`payments`]). The payments read the whole state, not the viewer's observation, but only
/// the costs and the viewer's own permanents decide them.
pub fn add_payments(view: &mut PlayerView, s: &State, t: &GameTable, d: &Decision) {
    if let Status::Deciding { decision } = &mut view.status {
        for (c, p) in decision.candidates.iter_mut().zip(payments(s, t, d)) {
            c.payments = p;
        }
    }
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
                (_, Opt::Cast { .. } | Opt::Activate { .. }) => Payment::default(),
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
            let mut copy = *s;
            // Every question the cast asks comes back as a decision, even one with a single
            // answer, so the walk sees where the cast ends.
            copy.set_profile(Profile {
                stops: Stops::Forced,
                ..s.profile()
            });
            let mut scratch = Scratch::new();
            let choice = Choice(u8::try_from(i).expect("at most 64 candidates"));
            apply(&mut copy, t, &mut scratch, &mut Replay, choice);
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
    while matches!(s.pending(), Pending::Casting(_)) {
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
                let choice = Choice(u8::try_from(i).expect("at most 64 candidates"));
                apply(&mut copy, t, scratch, &mut Replay, choice);
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
    use beryl_engine::Slot;
    use beryl_engine::card::Castings;
    use beryl_scenario::{P0, Permanent, Scenario};

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
}
