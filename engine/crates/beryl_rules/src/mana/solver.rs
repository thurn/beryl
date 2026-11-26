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
use crate::table::GameTable;

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
    /// The mana that remembers it spends, a bit per entry of [`Sources::remembered`]: none
    /// in a game whose cards make no restricted mana.
    pub(super) remembered: u8,
}

impl<L: Layout> Default for Plan<L> {
    fn default() -> Self {
        Plan {
            spend: [0; KINDS],
            taps: Table::filled(0),
            left: [0; KINDS],
            remembered: 0,
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
    if sources.has_memory() {
        walk::<L, true>(cost, total, pool, sources, 0, 0, &mut plan, f);
    } else {
        walk::<L, false>(cost, total, pool, sources, 0, 0, &mut plan, f);
    }
}

/// The plan search: positions `0..KINDS` choose floating mana to spend, the next whether to
/// spend each mana that remembers the payment accepts, and the rest how many sources of each
/// class to tap. `must` counts the units that must be spent so far. `MEMORY`: whether the
/// sources have a restricted class or the pool mana that remembers ([`Sources::has_memory`]),
/// so that a search of any other payment reads none of it.
#[allow(clippy::too_many_arguments)]
fn walk<L: Layout, const MEMORY: bool>(
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
    let remembered = if MEMORY {
        sources.remembered().len()
    } else {
        0
    };
    if position == KINDS + remembered + classes.len() {
        return match complete::<L, MEMORY>(cost, total, pool, sources, plan) {
            Some(left) if !wasteful::<L, MEMORY>(cost, total, pool, sources, plan) => {
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
            if !walk::<L, MEMORY>(
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
    if MEMORY && position < KINDS + remembered {
        let bit = 1 << (position - KINDS);
        for spent in [true, false] {
            if spent && must + 1 > total {
                continue;
            }
            plan.remembered = if spent {
                plan.remembered | bit
            } else {
                plan.remembered & !bit
            };
            let must = must + u32::from(spent);
            if !walk::<L, MEMORY>(cost, total, pool, sources, position + 1, must, plan, f) {
                return false;
            }
        }
        return true;
    }
    let i = position - KINDS - remembered;
    let class = classes[i];
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
        plan.taps[i] = n;
        let must = must + if class.amount == 1 { u32::from(n) } else { 0 };
        if !walk::<L, MEMORY>(cost, total, pool, sources, position + 1, must, plan, f) {
            return false;
        }
    }
    plan.taps[i] = 0;
    true
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
/// `cost` exactly.
fn complete<L: Layout, const MEMORY: bool>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    plan: &Plan<L>,
) -> Option<[u8; KINDS]> {
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
    let need = total.checked_sub(must)?;
    if several_taps > need || need > several_make || (several_taps == 0 && need > 0) {
        return None;
    }
    let fixed = fixed_units::<L, MEMORY>(sources, plan);
    let (spent, pays) = spent_by_class(sources, plan, need, cost, fixed);
    if !pays {
        return None;
    }
    let mut left = pool;
    for (l, &spent) in left.iter_mut().zip(&plan.spend) {
        *l -= spent;
    }
    for (i, c) in classes.iter().enumerate() {
        let made = u32::from(plan.taps[i]) * u32::from(c.amount);
        if made > spent[i] {
            // Each kind of floating mana is capped at 255, as in State::add_mana.
            let remainder = u8::try_from(made - spent[i]).unwrap_or(u8::MAX);
            let floating = &mut left[lowest_kind(c.kinds)];
            *floating = floating.saturating_add(remainder);
        }
    }
    Some(left)
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
) -> bool {
    (0..sources.classes().len()).any(|i| {
        plan.taps[i] > 0 && {
            plan.taps[i] -= 1;
            let fewer = complete::<L, MEMORY>(cost, total, pool, sources, plan).is_some();
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
/// - see [CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may activate
///   an activated mana ability whenever they have priority".
fn floats_for_nothing<L: Layout>(sources: &Sources<L>, p: &Plan<L>, q: &Plan<L>) -> bool {
    if p == q
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
fn offer<L: Layout, const MEMORY: bool>(
    sources: &Sources<L>,
    out: &mut L::Queue<Plan<L>, 64>,
    scores: &mut L::Queue<Option<(u32, usize, u32)>, 64>,
    score: &mut impl FnMut(&Plan<L>) -> (u32, usize, u32),
    p: &Plan<L>,
) -> bool {
    let found = if MEMORY {
        let outcome = sources.outcome(p);
        (out.iter()).position(|q| q.left == p.left && sources.outcome(q) == outcome)
    } else {
        let sacrificed = sources.sacrificed(p);
        (out.iter()).position(|q| q.left == p.left && sources.sacrificed(q) == sacrificed)
    };
    match found {
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
            false
        }
        None => out.push(p.clone()).is_err() || scores.push(None).is_err(),
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
    let pool = s.player(player).mana().amounts();
    let mut out: L::Queue<Plan<L>, 64> = Default::default();
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
        });
        if failed.get() {
            return Err(Full);
        }
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
    // Two searches, so that a payment that can spend no mana that remembers compares plans
    // without testing for it at each one.
    if sources.has_memory() {
        each_plan(cost, pool, &sources, &mut |p| {
            if accepted(p) {
                full |= offer::<L, true>(&sources, &mut out, &mut scores, &mut score, p);
            }
            !full
        });
    } else {
        each_plan(cost, pool, &sources, &mut |p| {
            if accepted(p) {
                full |= offer::<L, false>(&sources, &mut out, &mut scores, &mut score, p);
            }
            !full
        });
    }
    if full || failed.get() {
        return Err(Full);
    }
    // A plan is dominated only by one sacrificing fewer, so with one number sacrificed among
    // them all no score is needed and every plan is kept.
    let fewest = out.iter().map(|p| sources.sacrificed(p)).min();
    if out.iter().any(|p| Some(sources.sacrificed(p)) != fewest) {
        for (p, sc) in out.iter().zip(scores.as_mut_slice()) {
            sc.get_or_insert_with(|| score(p));
        }
        let mut dominated: L::Queue<bool, 64> = Default::default();
        for (p, &sc) in out.iter().zip(scores.iter()) {
            let sacrificed = sources.sacrificed(p);
            dominated.push(out.iter().zip(scores.iter()).any(|(q, &other)| {
                sources.sacrificed(q) < sacrificed && other.expect("scored") >= sc.expect("scored")
            }))?;
        }
        let mut i = 0;
        out.retain(|_| {
            i += 1;
            !dominated.as_slice()[i - 1]
        });
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
