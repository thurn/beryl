//! The payment plans offered: which sources a plan taps and which floating mana it spends.

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::State;
use crate::table::GameTable;

use super::lookahead::{Later, later_costs, looking_ahead, still_payable};
use super::sources::Sources;
use super::{CLASSES, KINDS, Taps, units::Units};

/// One way to pay a cost: the floating mana spent by kind, how many sources of each class
/// are tapped, and the mana left floating afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Plan<L: Layout = Small> {
    pub(super) spend: [u8; KINDS],
    pub(super) taps: Taps<L>,
    pub(crate) left: [u8; KINDS],
}

impl<L: Layout> Default for Plan<L> {
    fn default() -> Self {
        Plan {
            spend: [0; KINDS],
            taps: Table::filled(0),
            left: [0; KINDS],
        }
    }
}

/// Calls `f` with every plan that pays `cost` exactly with no source tapped in vain, in
/// greedy order: the most floating mana first, then the least flexible sources. `f`
/// returns false to stop.
pub(super) fn each_plan<L: Layout>(
    cost: ManaCost,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    f: &mut dyn FnMut(&Plan<L>) -> bool,
) {
    let total = cost.total(0);
    let mut plan = Plan::default();
    walk(cost, total, pool, sources, 0, 0, &mut plan, f);
}

/// The plan search: positions `0..KINDS` choose floating mana to spend, the rest how many
/// sources of each class to tap. `must` counts the units that must be spent so far.
#[allow(clippy::too_many_arguments)]
fn walk<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    position: usize,
    must: u32,
    plan: &mut Plan<L>,
    f: &mut dyn FnMut(&Plan<L>) -> bool,
) -> bool {
    let classes = sources.classes();
    if position == KINDS + classes.len() {
        return match complete(cost, total, pool, sources, plan) {
            Some(left) if !wasteful(cost, total, pool, sources, plan) => {
                plan.left = left;
                f(plan)
            }
            _ => true,
        };
    }
    if position < KINDS {
        for n in (0..=pool[position]).rev() {
            if must + u32::from(n) > total {
                continue;
            }
            plan.spend[position] = n;
            if !walk(
                cost,
                total,
                pool,
                sources,
                position + 1,
                must + u32::from(n),
                plan,
                f,
            ) {
                return false;
            }
        }
        plan.spend[position] = 0;
        return true;
    }
    let i = position - KINDS;
    let class = classes[i];
    for n in (0..=class.count()).rev() {
        // Every tapped source spends at least one of its mana.
        if must + u32::from(n) > total {
            continue;
        }
        plan.taps[i] = n;
        let must = must + if class.amount == 1 { u32::from(n) } else { 0 };
        if !walk(cost, total, pool, sources, position + 1, must, plan, f) {
            return false;
        }
    }
    plan.taps[i] = 0;
    true
}

/// How many units each tapped class spends under `plan`: all of a one-mana source's, and of
/// the sources that make several, `need` spread over them, at least one each.
pub(super) fn spent_by_class<L: Layout>(
    sources: &Sources<L>,
    plan: &Plan<L>,
    need: u32,
) -> L::Table<u32, CLASSES> {
    let mut spent: L::Table<u32, CLASSES> = Table::filled(0);
    let mut rest = need;
    for (i, c) in sources.classes().iter().enumerate() {
        let taps = u32::from(plan.taps[i]);
        if c.amount == 1 {
            spent[i] = taps;
        } else if taps > 0 {
            spent[i] = taps;
            rest -= taps;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        if c.amount > 1 && plan.taps[i] > 0 {
            let room = u32::from(plan.taps[i]) * u32::from(c.amount - 1);
            let more = room.min(rest);
            spent[i] += more;
            rest -= more;
        }
    }
    spent
}

/// Checks a plan's tap counts and floating spend: the mana it leaves floating when they pay
/// `cost` exactly.
fn complete<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &Plan<L>,
) -> Option<[u8; KINDS]> {
    let classes = sources.classes();
    let mut must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum();
    let mut several_taps = 0;
    let mut several_make = 0;
    for (i, c) in classes.iter().enumerate() {
        let taps = u32::from(plan.taps[i]);
        if c.amount == 1 {
            must += taps;
        } else {
            several_taps += taps;
            several_make += taps * u32::from(c.amount);
        }
    }
    let need = total.checked_sub(must)?;
    if several_taps > need || need > several_make || (several_taps == 0 && need > 0) {
        return None;
    }
    let spent = spent_by_class(sources, plan, need);
    let mut units = Units::NONE;
    for (k, &n) in plan.spend.iter().enumerate() {
        units.add(1 << k, u16::from(n));
    }
    for (c, &n) in classes.iter().zip(spent.iter()) {
        units.add(c.kinds, n as u16);
    }
    if !units.pays(cost) {
        return None;
    }
    let mut left = pool;
    for (l, &spent) in left.iter_mut().zip(&plan.spend) {
        *l -= spent;
    }
    for (i, c) in classes.iter().enumerate() {
        let made = u32::from(plan.taps[i]) * u32::from(c.amount);
        if made > spent[i] {
            left[lowest_kind(c.kinds)] += (made - spent[i]) as u8;
        }
    }
    Some(left)
}

/// Whether a paying plan taps a source it could leave untapped, spending the same floating
/// mana: three Plains and a Mana Crypt for {3}{W}, when the Crypt and two Plains pay it. The
/// extra source only turns into floating mana, which tapping it later would make as well, so
/// the plan is dominated (§10) and never offered or taken. The cube has no {C} symbol in a
/// cost, so colorless mana left floating is never worth more than the source's own mana.
fn wasteful<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &mut Plan<L>,
) -> bool {
    (0..sources.classes().len()).any(|i| {
        plan.taps[i] > 0 && {
            plan.taps[i] -= 1;
            let fewer = complete(cost, total, pool, sources, plan).is_some();
            plan.taps[i] += 1;
            fewer
        }
    })
}

/// Whether plan `p` taps sources only to leave mana floating that they would make as well
/// left untapped, as plan `q` leaves them: `q` taps some of what `p` taps, sacrifices no more,
/// and leaves less floating, by mana the sources `p` taps beyond it could make. A player who
/// floated {R} from a Mountain pays a Lightning Bolt with it, not with their other Mountain,
/// so `p` is dominated (§10) and never offered or taken: mana floats only until the step
/// ends, and an untapped source makes it whenever its controller next has priority.
///
/// - [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "Each player's mana pool
///   empties at the end of each step and phase".
/// - [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may activate
///   an activated mana ability whenever they have priority".
fn floats_for_nothing<L: Layout>(sources: &Sources<L>, p: &Plan<L>, q: &Plan<L>) -> bool {
    if p == q
        || q.taps.iter().zip(p.taps.iter()).any(|(a, b)| a > b)
        || q.left.iter().zip(&p.left).any(|(a, b)| a > b)
        || sources.sacrificed(q) > sources.sacrificed(p)
    {
        return false;
    }
    let mut extra = Units::NONE;
    for (i, c) in sources.classes().iter().enumerate() {
        extra.add(
            c.kinds,
            u16::from(c.amount) * u16::from(p.taps[i] - q.taps[i]),
        );
    }
    let mut more = [0; KINDS];
    for (m, (a, b)) in more.iter_mut().zip(p.left.iter().zip(&q.left)) {
        *m = a - b;
    }
    extra.pays(ManaCost::new(0, more))
}

pub(super) fn lowest_kind(kinds: u8) -> usize {
    kinds.trailing_zeros() as usize
}

/// The plans a payment of `cost` offers, at most one per distinct floating remainder and
/// number of sources sacrificed, in order of the fewest sacrificed, then the least mana left
/// floating. Sacrificing a source is a plan's own outcome (§10), but almost always a waste:
/// a plan that sacrifices more is offered only when it keeps strictly more later costs payable
/// than every plan sacrificing fewer. Under [`crate::profile::Payment::Greedy`] it is the
/// first plan in greedy order of those sacrificing the fewest, alone.
///
/// The human's payment ([`crate::Profile::taps_mana`]) offers the same plans, each tapping
/// the sources [`looking_ahead`] picks.
pub(crate) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    greedy: bool,
) -> Result<L::Queue<Plan<L>, 64>, Full> {
    sink.work.solver_runs += 1;
    let sources = Sources::of(s, t, player, exclude)?;
    let pool = s.player(player).mana().amounts();
    let mut out: L::Queue<Plan<L>, 64> = Default::default();
    if greedy {
        let mut best: Option<(u8, Plan<L>)> = None;
        each_plan(cost, pool, &sources, &mut |p| {
            let sacrificed = sources.sacrificed(p);
            if best.as_ref().is_none_or(|&(fewest, _)| sacrificed < fewest) {
                best = Some((sacrificed, p.clone()));
            }
            sacrificed > 0
        });
        if let Some((_, p)) = best {
            out.push(p)?;
        }
        return Ok(out);
    }
    // Scores decide only between plans of one remainder and number sacrificed, and whether a
    // plan sacrificing more is dominated, so each is found when one of those first needs it:
    // a payment with one way to leave each remainder scores none.
    let mut later: Option<L::Queue<Later<L>, 32>> = None;
    let mut score = |p: &Plan<L>| {
        let later = later.get_or_insert_with(|| later_costs(s, t, player));
        still_payable(&sources, p, later.as_slice())
    };
    let mut scores: L::Queue<Option<(u32, usize, u32)>, 64> = Default::default();
    let mut full = false;
    each_plan(cost, pool, &sources, &mut |p| {
        let sacrificed = sources.sacrificed(p);
        match out
            .iter()
            .position(|q| q.left == p.left && sources.sacrificed(q) == sacrificed)
        {
            // The first plan of a remainder taps the least flexible sources; a later one
            // replaces it only by keeping more costs payable.
            Some(i) => {
                let q = &out.as_slice()[i];
                let first = *scores.as_mut_slice()[i].get_or_insert_with(|| score(q));
                let this = score(p);
                if this > first {
                    out.as_mut_slice()[i] = p.clone();
                    scores.as_mut_slice()[i] = Some(this);
                }
            }
            None => {
                full |= out.push(p.clone()).is_err() || scores.push(None).is_err();
            }
        }
        !full
    });
    if full {
        return Err(Full);
    }
    // A plan is dominated only by one sacrificing fewer, so with one number sacrificed among
    // them all no score is needed.
    let fewest = out.iter().map(|p| sources.sacrificed(p)).min();
    if out.iter().any(|p| Some(sources.sacrificed(p)) != fewest) {
        for (p, sc) in out.iter().zip(scores.as_mut_slice()) {
            sc.get_or_insert_with(|| score(p));
        }
    }
    let mut kept: L::Queue<Plan<L>, 64> = Default::default();
    for (p, &sc) in out.iter().zip(scores.iter()) {
        let sacrificed = sources.sacrificed(p);
        let dominated = out.iter().zip(scores.iter()).any(|(q, &other)| {
            sources.sacrificed(q) < sacrificed && other.expect("scored") >= sc.expect("scored")
        });
        if !dominated {
            kept.push(p.clone())?;
        }
    }
    // Only a pool can leave mana floating that a source would make again: a source's own
    // leftover mana is `wasteful`'s.
    if kept.len() > 1 && pool.iter().any(|&n| n > 0) {
        let all = kept.clone();
        kept.retain(|p| !all.iter().any(|q| floats_for_nothing(&sources, p, q)));
    }
    kept.as_mut_slice().sort_by_key(|p| {
        let floating: u32 = p.left.iter().map(|&n| u32::from(n)).sum();
        (sources.sacrificed(p), floating, p.left)
    });
    if s.profile().taps_mana == Some(player) {
        looking_ahead(s, t, player, cost, &sources, kept.as_mut_slice())?;
    }
    Ok(kept)
}
