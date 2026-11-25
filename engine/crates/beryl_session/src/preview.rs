//! What a candidate would lead to, played out on a copy of the game: what a cast or an
//! activation would tap, so the UI can light the permanents a payment taps before the player
//! commits to it and tell payment plans apart by what they tap rather than by number; and which
//! attackers a blocker may block, so the UI can take a whole block declaration in any order
//! before it sends the engine's picks; and what a count of targets lets the first target be, so
//! the UI can take the count with the target.

use beryl_engine::card::{AbilityKind, CostAction, Purpose};
use beryl_engine::layout::Queue;
use beryl_engine::state::Pending;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Layout, Opt, Replay, Scratch, Seat,
    SlotId, State, Step, Zone, apply, cost_being_cast, next,
};

use crate::Status;
use crate::prompt::forced;
use crate::protocol::ObjectId;
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
fn blocks<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Vec<Vec<ObjectId>> {
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
                // The attackers offered, which are objects.
                Step::Decide(d) if d.kind == DecisionKind::BlockTarget => d
                    .object_candidates()
                    .map(|slot| ObjectId(slot.num()))
                    .collect(),
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
        if p == d.decider {
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
                Step::Decide(next) if next.kind == DecisionKind::Target => match &next.candidates {
                    Candidates::Objects(_) => next
                        .object_candidates()
                        .map(|slot| TargetRef::Object {
                            slot: ObjectId(slot.num()),
                        })
                        .collect(),
                    Candidates::Options(opts) => opts
                        .iter()
                        .filter_map(|o| match *o {
                            Opt::Player(p) => Some(TargetRef::Player { player: side(p) }),
                            // "Any target" lists the permanents it can choose after the players.
                            Opt::Object(slot) => Some(TargetRef::Object {
                                slot: ObjectId(slot.num()),
                            }),
                            Opt::Ability(place) => Some(TargetRef::Ability {
                                place: place.into(),
                            }),
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
    let mut before_scratch = Scratch::new();
    let before = if matches!(s.pending(), Pending::Resolving(_)) {
        beryl_engine::view(s, t, &mut before_scratch)
    } else {
        s
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
                (DecisionKind::YesNo, Opt::Yes) if d.prompt.purpose == Purpose::UnlessPays => {
                    return vec![unless_paid(s, t, d, i)];
                }
                _ => return Vec::new(),
            };
            // The walk sees where the cast ends.
            let at = priced(at, before, t);
            let mut copy = forced(s);
            let mut scratch = Scratch::new();
            apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
            let mut out = Vec::new();
            walk(before, copy, t, &mut scratch, &at, &mut out);
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
        && paying.prompt.purpose == Purpose::Mana
    {
        apply(&mut copy, t, &mut scratch, &mut Replay, Choice(0));
    }
    Payment {
        taps: tapped(s, &copy, t),
        ..Payment::default()
    }
}

/// Whether a cast or face-up payment is still in progress in `s`, including a cast
/// inside a paused resolution whose state is visible only in the replayed view.
fn casting_in_progress<L: Layout>(s: &State<L>, t: &GameTable, scratch: &mut Scratch<L>) -> bool {
    match s.pending() {
        Pending::Casting(_) | Pending::TurningFaceUp { .. } => true,
        Pending::Resolving(_) => {
            matches!(
                beryl_engine::view(s, t, scratch).pending(),
                Pending::Casting(_)
            )
        }
        _ => false,
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
    while casting_in_progress(&s, t, scratch) {
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
                let shown = beryl_engine::view(&s, t, scratch);
                let at = priced(branch(o).expect("one kind"), shown, t);
                let mut copy = s.clone();
                apply(&mut copy, t, scratch, &mut Replay, choice(i));
                walk(before, copy, t, scratch, &at, out);
            }
            return;
        }
        apply(&mut s, t, scratch, &mut Replay, Choice(0));
    }
    let after = beryl_engine::view(&s, t, scratch);
    out.push(Payment {
        taps: tapped(before, after, t),
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

/// The permanents untapped in `before` and tapped in `after`, by slot, with those the payment
/// sacrificed for mana, as a Treasure
/// ([CR 111.10a](https://yawgatog.com/resources/magic-rules/#R11110a)): "{T}, Sacrifice this
/// token: Add one mana of any color." A permanent sacrificed otherwise, as Strip Mine for its
/// own ability, paid no mana.
pub(crate) fn tapped<L: Layout>(
    before: &State<L>,
    after: &State<L>,
    t: &GameTable,
) -> Vec<ObjectId> {
    let untapped = |s: &State<L>, slot| {
        let o = s.object(slot);
        o.zone() == Zone::Battlefield && !o.is_tapped()
    };
    let sacrifices_for_mana = |slot: L::Slot| {
        let face = before.definition(slot).face(t);
        face.abilities
            .iter()
            .any(|a| a.kind() == AbilityKind::Mana && a.cost().does(CostAction::Sacrifice))
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
        .map(|slot| ObjectId(slot.num()))
        .collect()
}
