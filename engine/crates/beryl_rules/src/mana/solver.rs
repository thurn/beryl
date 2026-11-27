//! The payment plans offered: which sources a plan taps and which floating mana it spends.
//!
//! Implements: none. Enumerates mana payments for the engine to offer.

use std::cell::Cell;

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::{Capability, GameTable};

use super::lookahead::{Later, later_costs, looking_ahead, still_payable};
use super::remembered::Paying;
use super::sources::{Class, Sources};
use super::{CLASSES, KINDS, Taps, units::Units};

/// One way to pay a cost: the floating mana spent by kind, how many sources of each class
/// are tapped, and the mana left floating afterwards.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Plan<L: Layout = Small> {
    pub(super) spend: [u8; KINDS],
    pub(super) taps: Taps<L>,
    pub(crate) left: [u8; KINDS],
    /// The mana that remembers it spends, a bit per entry of [`Sources::remembered`].
    pub(super) remembered: u8,
    /// Priced or multi-term mana sequence (`mana::rich`), `None` for a plain plan.
    pub(super) sequence: Option<Box<super::rich::Sequence>>,
}

impl<L: Layout> Default for Plan<L> {
    fn default() -> Self {
        Plan {
            spend: [0; KINDS],
            taps: Table::filled(0),
            left: [0; KINDS],
            remembered: 0,
            sequence: None,
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
) -> Result<(), Full> {
    let total = cost.total(0);
    let pool_sum: u32 = pool.iter().map(|&n| u32::from(n)).sum();
    let mut rem = pool_sum;
    if sources.has_memory() {
        rem += u32::try_from(sources.remembered().len()).expect("remembered fits in u32");
    }
    for c in sources.classes() {
        rem += u32::from(c.count()) * u32::from(c.amount);
    }
    if rem < total {
        return Ok(());
    }
    let start = if pool_sum == 0 { KINDS } else { 0 };
    let mut plan = Plan::default();
    let w = if sources.has_memory() {
        walk::<L, true>
    } else {
        walk::<L, false>
    };
    w(cost, total, pool, sources, start, 0, 0, rem, &mut plan, f)?;
    Ok(())
}

/// The plan search: positions `0..KINDS` choose floating mana to spend, the next whether to
/// spend each mana that remembers the payment accepts, and the rest how many sources of each
/// class to tap. `must` counts the units that must be spent so far, `have_max` the most mana
/// chosen positions can make together, and `rem` an upper bound on mana from positions still to
/// visit. `MEMORY`: whether the sources have a restricted class or the pool mana that
/// remembers ([`Sources::has_memory`]), so that a search of any other payment reads none of it.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn walk<L: Layout, const MEMORY: bool>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    position: usize,
    must: u32,
    have_max: u32,
    rem: u32,
    plan: &mut Plan<L>,
    f: &mut dyn FnMut(&Plan<L>) -> bool,
) -> Result<bool, Full> {
    let classes = sources.classes();
    let remembered = if MEMORY {
        sources.remembered().len()
    } else {
        0
    };
    if position == KINDS + remembered + classes.len() || must == total {
        if !sources.has_several() {
            if must == total && super::units::exact_pays::<L, MEMORY>(cost, sources, plan) {
                let mut left = pool;
                for (l, &spent) in left.iter_mut().zip(&plan.spend) {
                    *l -= spent;
                }
                plan.left = left;
                return Ok(f(plan));
            }
            return Ok(true);
        }
        return match complete::<L, MEMORY>(cost, total, pool, sources, plan)? {
            Some((left, has_several))
                if !has_several || !wasteful::<L, MEMORY>(cost, total, pool, sources, plan)? =>
            {
                plan.left = left;
                Ok(f(plan))
            }
            _ => Ok(true),
        };
    }
    if position < KINDS {
        let next_rem = rem - u32::from(pool[position]);
        for n in (0..=pool[position]).rev() {
            if must + u32::from(n) > total {
                continue;
            }
            if have_max + u32::from(n) + next_rem < total {
                break;
            }
            plan.spend[position] = n;
            if !walk::<L, MEMORY>(
                cost,
                total,
                pool,
                sources,
                position + 1,
                must + u32::from(n),
                have_max + u32::from(n),
                next_rem,
                plan,
                f,
            )? {
                return Ok(false);
            }
        }
        plan.spend[position] = 0;
        return Ok(true);
    }
    if MEMORY && position < KINDS + remembered {
        let bit = 1 << (position - KINDS);
        let next_rem = rem - 1;
        for spent in [true, false] {
            let add = u32::from(spent);
            if must + add > total {
                continue;
            }
            if have_max + add + next_rem < total {
                break;
            }
            plan.remembered = if spent {
                plan.remembered | bit
            } else {
                plan.remembered & !bit
            };
            if !walk::<L, MEMORY>(
                cost,
                total,
                pool,
                sources,
                position + 1,
                must + add,
                have_max + add,
                next_rem,
                plan,
                f,
            )? {
                return Ok(false);
            }
        }
        plan.remembered &= !bit;
        return Ok(true);
    }
    let i = position - KINDS - remembered;
    let class = classes[i];
    let next_rem = rem - u32::from(class.count()) * u32::from(class.amount);
    // A permanent tapped for its other class's mana is tapped for this one's too.
    let room = match class.is_twin().then(|| class.twin()).flatten() {
        Some(twin) if MEMORY => class.count() - plan.taps[usize::from(twin)],
        _ => class.count(),
    };
    for n in (0..=room).rev() {
        // Every tapped source spends at least one of its mana.
        if must + u32::from(n) > total {
            continue;
        }
        let add_max = u32::from(n) * u32::from(class.amount);
        if have_max + add_max + next_rem < total {
            break;
        }
        plan.taps[i] = n;
        let next_must = must + if class.amount == 1 { u32::from(n) } else { 0 };
        if !walk::<L, MEMORY>(
            cost,
            total,
            pool,
            sources,
            position + 1,
            next_must,
            have_max + add_max,
            next_rem,
            plan,
            f,
        )? {
            return Ok(false);
        }
    }
    plan.taps[i] = 0;
    Ok(true)
}

/// The units `plan` spends that leave no choice: its floating mana, the mana that remembers it
/// spends and each tapped source of one mana. With `MEMORY`, as in [`walk`].
pub(super) fn fixed_units<L: Layout, const MEMORY: bool>(
    sources: &Sources<L>,
    plan: &Plan<L>,
) -> Units {
    let mut units = Units::NONE;
    for (k, &n) in plan.spend.iter().enumerate() {
        units.add(1 << k, u16::from(n));
    }
    if MEMORY {
        for (i, &(_, kind, _)) in sources.remembered().iter().enumerate() {
            if plan.remembered & (1 << i) != 0 {
                units.add(1 << kind, 1);
            }
        }
    }
    for (c, &taps) in sources.classes().iter().zip(plan.taps.iter()) {
        if c.amount == 1 {
            units.add(c.kinds, u16::from(taps));
        }
    }
    units
}

/// How many units each tapped class spends under `plan`, and whether those units with `fixed`
/// ([`fixed_units`]) pay `cost`: all of a one-mana source's, and of the sources that make
/// several, `need` spread over them, at least one each. The first spread tried gives each such
/// class in order as much as it can. Where the sources of several mana make different kinds,
/// the spread decides what they pay: Mana Crypt's {C}{C} and a sacrificed Svyelunite Temple's
/// {U}{U} pay {1}{U}{U} only with both of the Temple's mana spent. There each spread is tried
/// in turn until one pays, and the first is kept where none does.
pub(super) fn spent_by_class<L: Layout>(
    sources: &Sources<L>,
    plan: &Plan<L>,
    need: u32,
    cost: ManaCost,
    fixed: Units,
) -> (L::Table<u32, CLASSES>, bool) {
    let classes = sources.classes();
    let mut spent: L::Table<u32, CLASSES> = Table::filled(0);
    let mut rest = need;
    // The kinds of the tapped sources of several mana, and whether they differ.
    let mut several = None;
    let mut mixed = false;
    for (i, c) in classes.iter().enumerate() {
        let taps = u32::from(plan.taps[i]);
        if c.amount == 1 {
            spent[i] = taps;
        } else if taps > 0 {
            spent[i] = taps;
            rest -= taps;
            mixed |= several.is_some_and(|k| k != c.kinds);
            several = Some(c.kinds);
        }
    }
    let first = spread(classes, plan, 0, rest, &mut spent, &mut |spent| {
        pays_with(classes, spent, cost, fixed)
    });
    // Sources of several mana that all make one kind pay the same however it is spread.
    if first || !mixed {
        return (spent, first);
    }
    let mut tried = spent.clone();
    let mut any = |spent: &L::Table<u32, CLASSES>| pays_with(classes, spent, cost, fixed);
    if spread_all(classes, plan, 0, rest, &mut tried, &mut any) {
        return (tried, true);
    }
    (spent, false)
}

/// Gives each tapped class of several mana from `i` on as much of `rest` as it can make, and
/// says whether `fits` accepts the result. The spread [`spent_by_class`] tries first.
fn spread<L: Layout>(
    classes: &[Class<L>],
    plan: &Plan<L>,
    i: usize,
    mut rest: u32,
    spent: &mut L::Table<u32, CLASSES>,
    fits: &mut dyn FnMut(&L::Table<u32, CLASSES>) -> bool,
) -> bool {
    for (j, c) in classes.iter().enumerate().skip(i) {
        if c.amount > 1 && plan.taps[j] > 0 {
            let room = u32::from(plan.taps[j]) * u32::from(c.amount - 1);
            let more = room.min(rest);
            spent[j] += more;
            rest -= more;
        }
    }
    rest == 0 && fits(spent)
}

/// Tries every spread of `rest` over the tapped classes of several mana from `i` on, the most
/// to the earliest class first, until `fits` accepts one, which `spent` then holds.
fn spread_all<L: Layout>(
    classes: &[Class<L>],
    plan: &Plan<L>,
    i: usize,
    rest: u32,
    spent: &mut L::Table<u32, CLASSES>,
    fits: &mut dyn FnMut(&L::Table<u32, CLASSES>) -> bool,
) -> bool {
    let Some(j) = (i..classes.len()).find(|&j| classes[j].amount > 1 && plan.taps[j] > 0) else {
        return rest == 0 && fits(spent);
    };
    let room = u32::from(plan.taps[j]) * u32::from(classes[j].amount - 1);
    let base = u32::from(plan.taps[j]);
    for more in (0..=room.min(rest)).rev() {
        spent[j] = base + more;
        if spread_all(classes, plan, j + 1, rest - more, spent, fits) {
            return true;
        }
    }
    spent[j] = base;
    false
}

/// Whether `fixed` and the units the classes of several mana spend under `spent` pay `cost`.
fn pays_with<L: Layout>(
    classes: &[Class<L>],
    spent: &L::Table<u32, CLASSES>,
    cost: ManaCost,
    fixed: Units,
) -> bool {
    let mut units = fixed;
    for (c, &n) in classes.iter().zip(spent.iter()) {
        if c.amount > 1 {
            units.add(
                c.kinds,
                u16::try_from(n).expect("at most a byte of taps times a byte of mana"),
            );
        }
    }
    units.pays(cost)
}

/// Checks a plan's tap counts and floating spend: the mana it leaves floating when they pay
/// `cost` exactly, and whether the plan tapped any source making several mana.
fn complete<L: Layout, const MEMORY: bool>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &Plan<L>,
) -> Result<Option<([u8; KINDS], bool)>, Full> {
    let classes = sources.classes();
    let mut must: u32 = plan.spend.iter().map(|&n| u32::from(n)).sum::<u32>();
    if MEMORY {
        must += plan.remembered.count_ones();
    }
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
    let need = match total.checked_sub(must) {
        Some(n) if several_taps <= n && n <= several_make && (several_taps > 0 || n == 0) => n,
        _ => return Ok(None),
    };
    let subtract_spend = || {
        let mut left = pool;
        for (l, &spent) in left.iter_mut().zip(&plan.spend) {
            *l -= spent;
        }
        left
    };
    if several_taps == 0 {
        return Ok(super::units::exact_pays::<L, MEMORY>(cost, sources, plan)
            .then(|| (subtract_spend(), false)));
    }
    let fixed = fixed_units::<L, MEMORY>(sources, plan);
    let (spent, pays) = spent_by_class(sources, plan, need, cost, fixed);
    if !pays {
        return Ok(None);
    }
    let mut left = subtract_spend();
    for (i, c) in classes.iter().enumerate() {
        let made = u32::from(plan.taps[i]) * u32::from(c.amount);
        if made > spent[i] {
            let rem = u8::try_from(made - spent[i]).map_err(|_| Full)?;
            let fl = &mut left[lowest_kind(c.kinds)];
            *fl = fl.checked_add(rem).ok_or(Full)?;
        }
    }
    Ok(Some((left, true)))
}

/// Whether a paying plan taps a source it could leave untapped, spending the same floating
/// mana: three Plains and a Mana Crypt for {3}{W}, when the Crypt and two Plains pay it. The
/// extra source only turns into floating mana, which tapping it later would make as well, so
/// the plan is dominated (§10) and never offered or taken. No implemented card has a {C} symbol in a
/// cost, so colorless mana left floating is never worth more than the source's own mana.
// Inline in each copy of [`walk`], as it was in the one walk before there were two: called
// apart, it costs the rollout benchmark 1M instructions more.
#[allow(clippy::inline_always)]
#[inline(always)]
fn wasteful<L: Layout, const MEMORY: bool>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &mut Plan<L>,
) -> Result<bool, Full> {
    (0..sources.classes().len()).try_fold(false, |found, i| {
        if found || plan.taps[i] == 0 {
            return Ok(found);
        }
        plan.taps[i] -= 1;
        let fewer = complete::<L, MEMORY>(cost, total, pool, sources, plan)?.is_some();
        plan.taps[i] += 1;
        Ok(fewer)
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
/// - see [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may activate
///   an activated mana ability whenever they have priority".
fn floats_for_nothing<L: Layout>(sources: &Sources<L>, p: &Plan<L>, q: &Plan<L>) -> bool {
    if p == q
        || p.sequence.is_some()
        || q.sequence.is_some()
        || q.taps.iter().zip(p.taps.iter()).any(|(a, b)| a > b)
        || q.left.iter().zip(&p.left).any(|(a, b)| a > b)
        || sources.sacrificed(q) > sources.sacrificed(p)
        || (sources.has_memory()
            && (p.remembered != q.remembered || sources.rides(p) != sources.rides(q)))
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
    usize::try_from(kinds.trailing_zeros()).expect("a bit in a u8")
}

/// Offers the plan `p` to [`plans`]' list `out`, with `scores` beside it: a plan of the same
/// outcome as one listed replaces it only by keeping more later costs payable, which `score`
/// counts. Plans of one outcome leave the same mana floating and sacrifice as many sources;
/// with `MEMORY` (a payment that can spend mana that remembers), they also spend the same
/// such mana and give the spell the same rider ([`Sources::outcome`]). Returns whether `out`
/// is full.
type Score = (u32, usize, u32);
type Scored = Option<(Score, bool)>;

fn offer<L: Layout, const MEMORY: bool>(
    sources: &Sources<L>,
    out: &mut L::Queue<Plan<L>, 64>,
    outcomes: &mut L::Queue<(u8, bool), 64>,
    scores: &mut L::Queue<Scored, 64>,
    score: &mut impl FnMut(&Plan<L>) -> (Score, usize),
    p: &Plan<L>,
) -> bool {
    let sacrificed = sources.sacrificed(p);
    let rides = if MEMORY { sources.rides(p) } else { false };
    let found = if MEMORY {
        (out.iter().zip(outcomes.iter())).position(|(q, &(q_sac, q_rides))| {
            q.left == p.left
                && q_sac == sacrificed
                && q.remembered == p.remembered
                && q_rides == rides
        })
    } else {
        (out.iter().zip(outcomes.iter()))
            .position(|(q, &(q_sac, _))| q.left == p.left && q_sac == sacrificed)
    };
    match found {
        // The first plan of a remainder taps the least flexible sources; a later one
        // replaces it only by keeping more costs payable.
        Some(i) => {
            let q = &out.as_slice()[i];
            let (first, saturated) = *scores.as_mut_slice()[i].get_or_insert_with(|| {
                let (sc, n) = score(q);
                (sc, sc.1 == n)
            });
            if saturated {
                return false;
            }
            let (this, n) = score(p);
            if this > first {
                out.as_mut_slice()[i] = p.clone();
                scores.as_mut_slice()[i] = Some((this, this.1 == n));
            }
            false
        }
        None => {
            out.push(p.clone()).is_err()
                || outcomes.push((sacrificed, rides)).is_err()
                || scores.push(None).is_err()
        }
    }
}

/// The plans a payment of `cost` for what `paying` is for offers, at most one per distinct
/// floating remainder, number of sources sacrificed and restricted mana spent
/// ([`Sources::outcome`]), in order of the fewest sacrificed, then the least mana left
/// floating. Sacrificing a source is a plan's own outcome (§10), but almost always a waste:
/// a plan that sacrifices more is offered only when it keeps strictly more later costs payable
/// than every plan sacrificing fewer. Under [`crate::profile::Payment::Greedy`] it is the
/// first plan in greedy order of those sacrificing the fewest, alone.
///
/// The human's payment ([`crate::Profile::taps_mana`]) offers the same plans, each tapping
/// the sources [`looking_ahead`] picks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    greedy: bool,
    paying: Paying<L>,
) -> Result<L::Queue<Plan<L>, 64>, Full> {
    plans_accepting::<L, false>(s, t, sink, player, cost, exclude, greedy, paying, |_| {
        Ok(true)
    })
}

/// Discards a plan that consumes permanents needed for the rest of the casting cost
/// before scoring or deduplicating it. Plans that consume no sources need no such check.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) fn plans_accepting<L: Layout, const CHECK: bool>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    greedy: bool,
    paying: Paying<L>,
    mut accept: impl FnMut(&Plan<L>) -> Result<bool, Full>,
) -> Result<L::Queue<Plan<L>, 64>, Full> {
    sink.work.solver_runs += 1;
    let sources = Sources::of(s, t, player, exclude, paying)?;
    let failed = Cell::new(false);
    let mut accepted = |p: &Plan<L>| {
        if !CHECK || sources.sacrificed(p) == 0 {
            return true;
        }
        if let Ok(allowed) = accept(p) {
            allowed
        } else {
            failed.set(true);
            false
        }
    };
    let pool = s.player(player).mana().amounts_u8();
    let mut out: L::Queue<Plan<L>, 64> = Default::default();
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    if greedy {
        let mut best: Option<(u8, Plan<L>)> = None;
        each_plan(cost, pool, &sources, &mut |p| {
            if !accepted(p) {
                return true;
            }
            let sacrificed = sources.sacrificed(p);
            if best.as_ref().is_none_or(|&(fewest, _)| sacrificed < fewest) {
                best = Some((sacrificed, p.clone()));
            }
            sacrificed > 0
        })?;
        if failed.get() {
            return Err(Full);
        }
        if let Some((_, p)) = best {
            out.push(p)?;
        } else if rich {
            super::rich::plans(
                s,
                t,
                sink,
                (player, cost, exclude, paying),
                true,
                &mut accepted,
                &sources,
                &mut out,
            )?;
        }
        return if failed.get() { Err(Full) } else { Ok(out) };
    }
    let max_outcomes = if pool == [0; KINDS]
        && !sources.has_consumed()
        && !sources.has_several()
        && sources.remembered().is_empty()
        && sources.extras.is_empty()
    {
        Some(if sources.has_memory() && sources.has_rider() {
            2
        } else {
            1
        })
    } else {
        None
    };
    let rem_mana = max_outcomes.map(|_| {
        let unique: u32 = sources
            .classes()
            .iter()
            .filter(|c| !c.is_twin())
            .map(|c| u32::from(c.count()))
            .sum();
        // Heuristic upper bound on remaining single-mana sources; floors at 0.
        unique.saturating_sub(cost.total(0))
    });
    // Scores decide only between plans of one remainder and number sacrificed.
    let mut later: Option<L::Queue<Later<L>, 32>> = None;
    let mut score = |p: &Plan<L>| {
        let later = later.get_or_insert_with(|| {
            let mut l = later_costs(s, t, player);
            if let Some(rem) = rem_mana {
                l.retain(|item| item.cost.total(0) <= rem);
            }
            l
        });
        let slice = later.as_slice();
        (still_payable(&sources, p, slice), slice.len())
    };
    let mut outcomes: L::Queue<(u8, bool), 64> = Default::default();
    let mut scores: L::Queue<Scored, 64> = Default::default();
    let mut full = false;
    // Two searches: a payment that can spend no mana that remembers skips testing for it.
    if sources.has_memory() {
        each_plan(cost, pool, &sources, &mut |p| {
            if accepted(p) {
                full |= offer::<L, true>(
                    &sources,
                    &mut out,
                    &mut outcomes,
                    &mut scores,
                    &mut score,
                    p,
                );
                if max_outcomes == Some(out.len()) {
                    let all_sat = scores.as_mut_slice().iter_mut().enumerate().all(|(i, sc)| {
                        let (_, sat) = *sc.get_or_insert_with(|| {
                            let (v, n) = score(&out.as_slice()[i]);
                            (v, v.1 == n)
                        });
                        sat
                    });
                    if all_sat {
                        return false;
                    }
                }
            }
            !full
        })?;
    } else {
        each_plan(cost, pool, &sources, &mut |p| {
            if accepted(p) {
                full |= offer::<L, false>(
                    &sources,
                    &mut out,
                    &mut outcomes,
                    &mut scores,
                    &mut score,
                    p,
                );
                if max_outcomes == Some(out.len()) {
                    let all_sat = scores.as_mut_slice().iter_mut().enumerate().all(|(i, sc)| {
                        let (_, sat) = *sc.get_or_insert_with(|| {
                            let (v, n) = score(&out.as_slice()[i]);
                            (v, v.1 == n)
                        });
                        sat
                    });
                    if all_sat {
                        return false;
                    }
                }
            }
            !full
        })?;
    }
    if full || failed.get() {
        return Err(Full);
    }
    if rich {
        super::rich::plans(
            s,
            t,
            sink,
            (player, cost, exclude, paying),
            false,
            &mut accepted,
            &sources,
            &mut out,
        )?;
        if failed.get() {
            return Err(Full);
        }
        while scores.len() < out.len() {
            scores.push(None)?;
        }
    }
    // A plan is dominated only by one sacrificing fewer.
    let fewest = out.iter().map(|p| sources.sacrificed(p)).min();
    if out.iter().any(|p| Some(sources.sacrificed(p)) != fewest) {
        for (p, sc) in out.iter().zip(scores.as_mut_slice()) {
            sc.get_or_insert_with(|| {
                let (s, n) = score(p);
                (s, s.1 == n)
            });
        }
        let mut dominated: L::Queue<bool, 64> = Default::default();
        for (p, &sc) in out.iter().zip(scores.iter()) {
            let sacrificed = sources.sacrificed(p);
            let sc = sc.expect("scored").0;
            dominated.push(out.iter().zip(scores.iter()).any(|(q, &other)| {
                sources.sacrificed(q) < sacrificed && other.expect("scored").0 >= sc
            }))?;
        }
        let mut keep = dominated.iter();
        out.retain(|_| !*keep.next().expect("scored"));
    }
    let mut kept = out;
    // Only a pool can leave mana floating that a source would make again: a source's own
    // leftover mana is `wasteful`'s.
    if kept.len() > 1 && pool.iter().any(|&n| n > 0) {
        let all = kept.clone();
        kept.retain(|p| !all.iter().any(|q| floats_for_nothing(&sources, p, q)));
    }
    let floating = |p: &Plan<L>| p.left.iter().map(|&n| u32::from(n)).sum::<u32>();
    if sources.has_memory() {
        // A rider first among plans alike but for it: the spell gets it at no cost but the
        // restricted mana.
        kept.as_mut_slice().sort_by_key(|p| {
            let rides = !sources.rides(p);
            (
                sources.sacrificed(p),
                floating(p),
                p.left,
                rides,
                p.remembered,
            )
        });
    } else {
        kept.as_mut_slice()
            .sort_by_key(|p| (sources.sacrificed(p), floating(p), p.left));
    }
    if s.profile().taps_mana == Some(player) {
        looking_ahead(s, t, player, cost, &sources, kept.as_mut_slice(), accepted)?;
        if failed.get() {
            return Err(Full);
        }
    }
    Ok(kept)
}
