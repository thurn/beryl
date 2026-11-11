//! A cost the engine takes one permanent at a time, worked out in the game: what each
//! permanent could pay toward it, which sets of picks pay without a wasted tap, and how the
//! default would pay what the picks leave.
//!
//! [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
//! total cost. First, they pay all costs that don't involve random elements or moving objects
//! from the library to a public zone, in any order." The engine asks a cost paid with
//! permanents as a run of picks: a mana ability to activate
//! ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "they then have a
//! chance to activate mana abilities"), a permanent to tap, an end where mana pays the rest,
//! and then the payment plan where plans differ. What a set of picks comes to is the
//! engine's to say, so each is played ahead on a copy of the game.

use std::collections::BTreeMap;

use beryl_engine::card::{ManaColor, ManaCost, Part, Purpose, Types};
use beryl_engine::{
    Choice, Decision, GameTable, Layout, Opt, Replay, Scratch, SlotId as _, State, Step, apply,
    interchangeable, mana_contribution, next,
};

use super::Outlook;
use crate::preview::tapped;
use crate::prompt::ahead::forced;
use crate::prompt::held::{self, Run, Want, options};
use crate::protocol::mana_text;
use crate::script::Subject;

/// A mana ability to activate: its permanent and the color of mana it makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Activation {
    slot: u16,
    color: ManaColor,
}

/// One way a permanent can pay toward the cost: by its mana, or by being tapped where the
/// cost takes a tap in place of mana
/// ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a): "you may tap an
/// untapped artifact or creature you control rather than pay that mana") or counts what the
/// tapped permanent has
/// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a): "Tap any number of
/// other untapped creatures you control with total power N or greater").
#[derive(Clone, Debug)]
struct Contribution {
    mana: Option<Activation>,
    amount: u16,
    /// Paying this way uses the permanent up: it is sacrificed for its mana.
    consumed: bool,
}

/// A cost as its prompt opens.
#[derive(Clone, Debug)]
pub(super) struct Due {
    purpose: Purpose,
    /// How much the picks pay toward: the units of the cost that are not paid already.
    need: u16,
    /// The ways each permanent that could pay can, by slot.
    sources: BTreeMap<u16, Vec<Contribution>>,
    /// Mana pays what the picks leave, by the engine's payment plan. Otherwise permanents
    /// alone pay.
    mana_pays: bool,
}

/// Whether the engine offers the permanent in `offered` for the one in `wanted` too: of
/// interchangeable permanents it offers one (architecture §7.2).
fn alike<L: Layout>(s: &State<L>, offered: L::Slot, wanted: L::Slot) -> bool {
    offered == wanted || interchangeable(s, offered, wanted)
}

/// The candidate at `i` as a choice.
fn choice(i: usize) -> Choice {
    Choice(u16::try_from(i).expect("at most 64 candidates"))
}

/// Whether `cost` is of mana; otherwise the prompt counts it.
fn of_mana(cost: ManaCost) -> bool {
    cost != ManaCost::default()
}

/// How many units are left to pay at `d`, a pick of the payment in `s`: for a cost the
/// prompt counts, what its part still takes; for mana, each symbol of what is owed.
fn left<L: Layout>(s: &State<L>, d: &Decision<L>) -> u16 {
    if !of_mana(d.prompt.cost) {
        return u16::from(d.prompt.most);
    }
    let rest = owed(s, d);
    u16::from(rest.generic) + rest.symbols.iter().map(|&n| u16::from(n)).sum::<u16>()
}

/// The mana cost `d`, a pick of the payment in `s`, still asks for, less the mana floating
/// that pays any of it: floating mana of any kind pays generic mana, and which colored
/// symbols it pays is the engine's to say.
fn owed<L: Layout>(s: &State<L>, d: &Decision<L>) -> ManaCost {
    let mut rest = d.prompt.cost;
    if rest.symbols.iter().all(|&n| n == 0) {
        let pool: u16 = (s.player(d.player).mana().amounts().into_iter())
            .map(u16::from)
            .sum();
        rest.generic = u8::try_from(u16::from(rest.generic).saturating_sub(pool)).unwrap_or(0);
    }
    rest
}

impl Due {
    /// The cost `first`, the open decision in `before`, begins to pay.
    pub(super) fn of<L: Layout>(before: &State<L>, t: &GameTable, first: &Decision<L>) -> Due {
        let offered = options(first);
        let taps = offered.iter().any(|o| matches!(o, Opt::Object(_)));
        let cost = first.prompt.cost;
        let units = left(before, first);
        // A permanent tapped in place of mana pays generic mana alone.
        let colored: u16 = cost.symbols.iter().map(|&n| u16::from(n)).sum();
        let need = if taps && of_mana(cost) {
            units.saturating_sub(colored)
        } else {
            units
        };
        let mut sources = BTreeMap::new();
        for &slot in before.battlefield() {
            let mut roles = Vec::new();
            let color = offered.iter().find_map(|o| match *o {
                Opt::Mana { slot: s, color } if alike(before, s, slot) => Some(color),
                _ => None,
            });
            if let Some(color) = color
                && let Some((amount, consumed)) = mana_contribution(before, t, first.player, slot)
            {
                roles.push(Contribution {
                    mana: Some(Activation {
                        slot: slot.num(),
                        color,
                    }),
                    amount: amount.into(),
                    consumed,
                });
            }
            let tapped = (offered.iter())
                .position(|o| matches!(*o, Opt::Object(s) if alike(before, s, slot)));
            if let Some(i) = tapped {
                let amount = if of_mana(cost) {
                    1
                } else {
                    counted(before, t, first, i, units)
                };
                if amount > 0 {
                    roles.push(Contribution {
                        mana: None,
                        amount,
                        consumed: false,
                    });
                }
            }
            if !roles.is_empty() {
                sources.insert(slot.num(), roles);
            }
        }
        Due {
            purpose: first.prompt.purpose,
            need,
            sources,
            mana_pays: offered
                .iter()
                .any(|o| matches!(o, Opt::Done | Opt::Mana { .. } | Opt::Pool(_))),
        }
    }

    /// Whether any permanent could pay toward the cost.
    pub(super) fn is_payable(&self) -> bool {
        !self.sources.is_empty() || self.mana_pays
    }
}

/// How much of the `units` a counted cost still takes the pick at `i` of `first`, the open
/// decision in `before`, pays: what the engine's next pick says is left, taken away.
fn counted<L: Layout>(
    before: &State<L>,
    t: &GameTable,
    first: &Decision<L>,
    i: usize,
    units: u16,
) -> u16 {
    let mut copy = forced(before);
    let mut scratch = Scratch::new();
    apply(&mut copy, t, &mut scratch, &mut Replay, choice(i));
    match next(&mut copy, t, &mut scratch, &mut Replay) {
        Step::Decide(d) if d.prompt.purpose == first.prompt.purpose => {
            units.saturating_sub(left(&copy, &d))
        }
        _ => units,
    }
}

type Selection = (u8, u8, Vec<Contribution>);

/// Finds a way for each of `selected` to pay toward `need`. Each row keeps the best way to
/// reach a total with a given smallest contribution. A set can overpay with a source of
/// several units only when omitting even its smallest contribution would leave it short.
fn roles(
    selected: &[u16],
    sources: &BTreeMap<u16, Vec<Contribution>>,
    need: u16,
) -> Option<Vec<Contribution>> {
    if selected.len() > usize::from(need) {
        return None;
    }
    let mut ways = BTreeMap::from([((0, u16::MAX), (0, 0, Vec::new()))]);
    for (i, slot) in selected.iter().enumerate() {
        if selected[..i].contains(slot) {
            return None;
        }
        let mut next: BTreeMap<_, Selection> = BTreeMap::new();
        for (&(total, least), (consumed, tapped, plan)) in &ways {
            for role in sources.get(slot)? {
                let total = total + role.amount;
                let least = least.min(role.amount);
                if total - least >= need {
                    continue;
                }
                let score = (
                    consumed + u8::from(role.consumed),
                    tapped + u8::from(role.mana.is_none()),
                );
                let entry = next.entry((total, least)).or_insert_with(|| {
                    let mut plan = plan.clone();
                    plan.push(role.clone());
                    (score.0, score.1, plan)
                });
                if score < (entry.0, entry.1) {
                    entry.0 = score.0;
                    entry.1 = score.1;
                    entry.2.clone_from(plan);
                    entry.2.push(role.clone());
                }
            }
        }
        ways = next;
    }
    ways.into_iter()
        .min_by_key(|&((total, _), (consumed, tapped, _))| {
            (need.saturating_sub(total), consumed, tapped, total)
        })
        .map(|(_, (_, _, roles))| roles)
}

/// The way each of `selected` pays toward `need` where permanents alone pay, each by being
/// tapped: none once the picks before the last pay it all, as the engine asks for no more.
fn in_turn(
    selected: &[u16],
    sources: &BTreeMap<u16, Vec<Contribution>>,
    need: u16,
) -> Option<Vec<Contribution>> {
    let mut total = 0;
    let mut roles = Vec::new();
    for (i, slot) in selected.iter().enumerate() {
        if selected[..i].contains(slot) || total >= need {
            return None;
        }
        let role = (sources.get(slot)?.iter()).find(|role| role.mana.is_none())?;
        total += role.amount;
        roles.push(role.clone());
    }
    Some(roles)
}

/// The permanents that tapped pay `remaining` where permanents alone pay, beyond those of
/// `picked`: the set that costs their controller least, creatures that could not attack this
/// turn before those that could, and as little attacking power and as few permanents as
/// cover it
/// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026): "A creature can't attack
/// unless it has been under its controller's control continuously since their most recent
/// turn began"; tapping one for another permanent's cost is not an ability of its own, so
/// nothing stops it).
fn cover<L: Layout>(
    before: &State<L>,
    due: &Due,
    picked: &[u16],
    remaining: u16,
) -> Option<Vec<u16>> {
    let forgone = |slot: u16| {
        let Some(slot) = L::Slot::from_num(slot) else {
            return 0;
        };
        let attacks = before.derived(slot).types.contains(Types::CREATURE)
            && !before.object(slot).is_summoning_sick();
        if attacks {
            u16::try_from(before.derived(slot).power.max(0)).unwrap_or(u16::MAX)
        } else {
            0
        }
    };
    // By the total reached, capped at what is left: what it costs, how many it taps, which.
    let mut best = BTreeMap::from([(0, ((0, 0), Vec::new()))]);
    for (&slot, roles) in &due.sources {
        let Some(role) = roles.iter().find(|role| role.mana.is_none()) else {
            continue;
        };
        if picked.contains(&slot) {
            continue;
        }
        for (total, ((price, count), set)) in best.clone() {
            if total >= remaining {
                continue;
            }
            let reached = remaining.min(total + role.amount);
            let cost = (price + forgone(slot), count + 1);
            if best.get(&reached).is_none_or(|(least, _)| cost < *least) {
                let mut set = set;
                set.push(slot);
                best.insert(reached, (cost, set));
            }
        }
    }
    best.remove(&remaining).map(|(_, set)| set)
}

/// What the picks of `picked` come to for the cost `due`, begun at `first`, the open
/// decision in `before`: none if they do not pay toward it without a wasted pick.
pub(super) fn outlook<L: Layout>(
    before: &State<L>,
    t: &GameTable,
    first: &Decision<L>,
    due: &Due,
    picked: &[u16],
) -> Option<Outlook> {
    let ways = |picked: &[u16]| {
        if due.mana_pays {
            roles(picked, &due.sources, due.need)
        } else {
            in_turn(picked, &due.sources, due.need)
        }
    };
    // The picks as the engine takes them, each permanent's mana before any is tapped
    // otherwise, if it takes them all: whatever pays the rest.
    let taken = |picked: &[u16]| {
        let roles = ways(picked)?;
        let activations: Vec<_> = roles.iter().filter_map(|r| r.mana).collect();
        let taps: Vec<_> = (picked.iter().zip(&roles))
            .filter_map(|(&slot, role)| role.mana.is_none().then_some(slot))
            .collect();
        line(before, t, first, &taps, &activations, None)?;
        Some((roles, activations, taps))
    };
    let (roles, activations, mut taps) = taken(picked)?;
    let mut eligible = Vec::new();
    let mut extra = picked.to_vec();
    for &slot in due.sources.keys().filter(|slot| !picked.contains(slot)) {
        extra.push(slot);
        if taken(&extra).is_some() {
            eligible.push(slot);
        }
        extra.pop();
    }
    let remaining = (roles.iter()).fold(due.need, |left, role| left.saturating_sub(role.amount));
    let line = if due.mana_pays {
        automatic(before, t, first, &taps, &activations, remaining)
    } else {
        cover(before, due, picked, remaining).and_then(|more| {
            taps.extend(more);
            line(before, t, first, &taps, &activations, Some(0))
        })
    };
    let left = unpaid(before, first, &roles, remaining);
    let Some(line) = line else {
        return Some(Outlook {
            eligible,
            suggested: Vec::new(),
            left,
            paid: false,
            run: None,
        });
    };
    let run = line.run(due.purpose);
    Some(Outlook {
        eligible,
        // Of interchangeable permanents the engine taps the one it offers: the one picked
        // shows as paying, and the one tapped for it is not the default's.
        suggested: (line.tapped.iter().copied())
            .filter(|slot| !picked.contains(slot) && !line.stood.contains(slot))
            .collect(),
        paid: line.tapped.len() == picked.len(),
        left,
        run: Some(run),
    })
}

/// What `roles`, the ways the permanents picked pay, leave of the cost begun at `first` in
/// `before`, as the screen writes it: for a cost the prompt counts, the `remaining` of it;
/// for mana, the cost less each unit paid, a unit of mana paying a symbol of its own kind
/// before generic mana, and a permanent tapped in place of mana paying generic mana.
fn unpaid<L: Layout>(
    before: &State<L>,
    first: &Decision<L>,
    roles: &[Contribution],
    remaining: u16,
) -> String {
    if !of_mana(first.prompt.cost) {
        return remaining.to_string();
    }
    let mut rest = owed(before, first);
    for role in roles {
        let kind = role.mana.map(|activation| activation.color.index());
        for _ in 0..role.amount {
            match kind {
                Some(kind) if rest.symbols[kind] > 0 => rest.symbols[kind] -= 1,
                _ => rest.generic = rest.generic.saturating_sub(1),
            }
        }
    }
    mana_text(rest)
}

/// A whole payment played ahead: the picks that make it, and what it taps.
struct Line {
    /// The picks of the cost's own prompt, in the engine's order.
    steps: Vec<held::Step>,
    /// The payment plan chosen where the engine asks for one after them.
    plan: Option<u16>,
    activations: Vec<Activation>,
    /// The permanents tapped by a pick of theirs.
    tappers: Vec<u16>,
    /// Those of them the engine took in place of a permanent picked.
    stood: Vec<u16>,
    /// Everything the payment taps or sacrifices for mana.
    tapped: Vec<u16>,
}

impl Line {
    /// Notes the pick of `option`, made for the permanent in `picked` if for one picked.
    fn take<L: Layout>(&mut self, option: Opt<L>, picked: Option<u16>) {
        let pick = |want: Want| held::Step::One(Part::Pick, want);
        match option {
            Opt::Mana { slot, color } => {
                let slot = slot.num();
                self.activations.push(Activation { slot, color });
                self.steps.push(pick(Want::Mana { slot, color }));
            }
            Opt::Pool(color) => self.steps.push(pick(Want::Pool(color))),
            Opt::Object(slot) => {
                self.tappers.push(slot.num());
                let on = Subject::Object { slot: slot.num() };
                self.steps.push(held::Step::Among(Part::Pick, vec![on]));
            }
            Opt::Done => self.steps.push(pick(Want::Done)),
            Opt::Plan(plan) => self.plan = u16::try_from(Into::<usize>::into(plan)).ok(),
            _ => {}
        }
        // The permanent the engine takes in place of the one picked.
        if let (Opt::Mana { slot, .. } | Opt::Object(slot), Some(picked)) = (option, picked)
            && slot.num() != picked
        {
            self.stood.push(slot.num());
        }
    }

    /// The run of picks that makes the payment, for a prompt of `purpose`: its own picks,
    /// then the payment plan where the engine asks for one.
    fn run(&self, purpose: Purpose) -> Run {
        let plan = (self.plan).map(|plan| {
            Run::new(
                Purpose::Mana,
                [held::Step::One(Part::Plan, Want::Plan(plan))],
            )
        });
        Run::new(purpose, self.steps.iter().cloned()).over(plan)
    }
}

/// The default's payment of what `taps` and `activations` leave of the cost begun at
/// `first`, `remaining` units of it: mana pays the rest. It taps no artifact or creature the
/// player did not pick, but as a permanent tapped in place of mana where its own mana would
/// use it up.
fn automatic<L: Layout>(
    before: &State<L>,
    t: &GameTable,
    first: &Decision<L>,
    taps: &[u16],
    activations: &[Activation],
    remaining: u16,
) -> Option<Line> {
    let explicit = |slot| {
        before
            .derived(slot)
            .types
            .intersects(Types::ARTIFACT.union(Types::CREATURE))
    };
    let safe = |line: &Line| {
        before.battlefield().iter().copied().all(|slot| {
            let n = slot.num();
            !line.tapped.contains(&n)
                || line.tappers.contains(&n)
                || line.activations.iter().any(|m| m.slot == n)
                || before.controller(slot) != first.player
                || !explicit(slot)
        })
    };
    let plan = line(before, t, first, taps, activations, Some(remaining))?;
    if safe(&plan) {
        return Some(plan);
    }

    // The usual mana plan can prefer a mana rock. Keep what the permanents tapped in place of
    // mana pay, and try funding the rest with ordinary sources that require no manual pick.
    let mut funded = activations.to_vec();
    let spared = u16::try_from(plan.tappers.len() - taps.len()).ok()?;
    let mut left = remaining.saturating_sub(spared);
    for option in options(first) {
        let Opt::Mana { slot, color } = option else {
            continue;
        };
        if explicit(slot) || funded.iter().any(|m| m.slot == slot.num()) {
            continue;
        }
        let Some((amount, false)) = mana_contribution(before, t, first.player, slot) else {
            continue;
        };
        if left == 0 {
            break;
        }
        left = left.saturating_sub(u16::from(amount));
        funded.push(Activation {
            slot: slot.num(),
            color,
        });
        let plan = line(before, t, first, taps, &funded, Some(remaining))?;
        if safe(&plan) {
            return Some(plan);
        }
    }
    None
}

/// Plays the payment begun at `first`, the open decision in `before`, on a copy: floating
/// mana first, then each of `activations`, then each of `taps` (or a permanent the engine
/// offers in its place), and then, given `spare`, the default for what is left: up to
/// `spare` permanents tapped in place of the mana they would be sacrificed for, the end,
/// where mana pays the rest, and the first payment plan. None if a pick cannot be made, or
/// permanents the default will not choose must still be tapped.
fn line<L: Layout>(
    before: &State<L>,
    t: &GameTable,
    first: &Decision<L>,
    taps: &[u16],
    activations: &[Activation],
    mut spare: Option<u16>,
) -> Option<Line> {
    let purpose = first.prompt.purpose;
    let mut copy = forced(before);
    let mut scratch = Scratch::new();
    let mut decision = first.clone();
    let mut line = Line {
        steps: Vec::new(),
        plan: None,
        activations: Vec::new(),
        tappers: Vec::new(),
        stood: Vec::new(),
        tapped: Vec::new(),
    };
    let mut made = 0;
    loop {
        let offered = options(&decision);
        let find = |wanted: &dyn Fn(Opt<L>) -> bool| offered.iter().position(|&o| wanted(o));
        let paying = decision.prompt.purpose == purpose && decision.prompt.part == Part::Pick;
        // The candidate taken, and the permanent picked that it is taken for, if one.
        let (index, picked) = if !paying {
            // The plan that follows a cost paid by tapping is the default's.
            if decision.prompt.purpose != Purpose::Mana || decision.prompt.part != Part::Plan {
                break;
            }
            (0, None)
        } else if let Some(i) = find(&|o| matches!(o, Opt::Pool(_))) {
            // Mana floating pays before anything is tapped for more.
            (i, None)
        } else if let Some(activation) = activations.get(line.activations.len()) {
            let wanted = L::Slot::from_num(activation.slot)?;
            let made = |o| {
                matches!(o, Opt::Mana { slot, color }
                    if activation.color == color && alike(before, slot, wanted))
            };
            (find(&made)?, Some(activation.slot))
        } else if let Some(&picked) = taps.get(made) {
            let wanted = L::Slot::from_num(picked)?;
            made += 1;
            let tapped = |o| matches!(o, Opt::Object(slot) if alike(before, slot, wanted));
            (find(&tapped)?, Some(picked))
        } else {
            // The picks are made: the rest is the default's, where it is wanted.
            let Some(spare) = &mut spare else {
                break;
            };
            let spared = |o: Opt<L>| {
                matches!(o, Opt::Object(slot) if *spare > 0
                    && mana_contribution(&copy, t, decision.player, slot)
                        .is_some_and(|(_, consumed)| consumed))
            };
            let index = if let Some(i) = find(&spared) {
                // Tapped in place of mana, the permanent stays on the battlefield.
                *spare -= 1;
                i
            } else if let Some(i) = find(&|o| o == Opt::Done) {
                i
            } else if (offered.iter()).all(|o| matches!(o, Opt::Mana { .. } | Opt::Pool(_))) {
                // Each unit paid by hand: every source offered leaves the rest payable.
                0
            } else {
                return None;
            };
            (index, None)
        };
        line.take(*offered.get(index)?, picked);
        apply(&mut copy, t, &mut scratch, &mut Replay, choice(index));
        match next(&mut copy, t, &mut scratch, &mut Replay) {
            Step::Decide(next) => decision = next,
            Step::Over(_) => break,
        }
    }
    if taps.len() != made || activations.len() > line.activations.len() {
        return None;
    }
    line.tapped = tapped(before, &copy, t);
    Some(line)
}
