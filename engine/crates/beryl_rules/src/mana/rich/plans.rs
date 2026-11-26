//! Sequence plan search for priced and multi-term mana abilities.
//!
//! Implements: none. Enumerates non-wasteful payment sequences for `mana::rich`.

use std::collections::BTreeSet;

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::GameTable;

use super::super::remembered::Paying;
use super::super::solver::Plan;
use super::super::sources::Sources;
use super::super::units::Units;
use super::super::{CLASSES, KINDS, Taps};
use super::{Sequence, Step, pays_from_state, pays_with_sources};

/// Adds to `out` the plans of a payment of `cost` by `player` with none of `exclude`, for what
/// `paying` is for, that activate the sources the classes of `sources` cannot say
/// (`Sources::extras`), each as [`super::super::plans_accepting`] offers a plan: kept only where
/// `accepted` takes it, and one for each outcome. Under `greedy`, the first such plan alone,
/// and only where `out` holds none.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(in crate::mana) fn plans<L: Layout>(
    s: &State<L>,
    _t: &GameTable,
    _sink: &mut Sink<L>,
    (player, cost, _exclude, _paying): (Seat, ManaCost, Bitboard<L::Slot>, Paying<L>),
    greedy: bool,
    accepted: &mut impl FnMut(&Plan<L>) -> bool,
    sources: &Sources<L>,
    out: &mut L::Queue<Plan<L>, 64>,
) -> Result<(), Full> {
    if sources.extras.is_empty() {
        return Ok(());
    }
    if greedy && !out.is_empty() {
        return Ok(());
    }
    if !pays_with_sources(s, player, cost, sources) {
        return Ok(());
    }
    let init_pool = s.player(player).mana().amounts();
    let mut ctx = PlanSearch {
        init_pool,
        cost,
        greedy,
        sources,
        accepted,
        out,
        seen: BTreeSet::new(),
        full: false,
        stop: false,
    };
    let state = SearchState {
        pool: init_pool,
        taps: Table::filled(0),
        extra_used: [0; CLASSES],
        plain: Vec::new(),
        steps: Vec::new(),
    };
    search_plans(&mut ctx, &state);
    if ctx.full { Err(Full) } else { Ok(()) }
}

struct PlanSearch<'a, L: Layout, F: FnMut(&Plan<L>) -> bool> {
    init_pool: [u8; KINDS],
    cost: ManaCost,
    greedy: bool,
    sources: &'a Sources<L>,
    accepted: &'a mut F,
    out: &'a mut L::Queue<Plan<L>, 64>,
    seen: BTreeSet<([u8; CLASSES], Vec<u8>, [u8; KINDS])>,
    full: bool,
    stop: bool,
}

#[derive(Clone)]
struct SearchState<L: Layout> {
    pool: [u8; KINDS],
    taps: Taps<L>,
    extra_used: [u8; CLASSES],
    plain: Vec<(u8, u8)>,
    steps: Vec<Step>,
}

fn search_plans<L: Layout, F: FnMut(&Plan<L>) -> bool>(
    ctx: &mut PlanSearch<'_, L, F>,
    st: &SearchState<L>,
) {
    if ctx.stop || ctx.full {
        return;
    }
    let taps_key: Vec<u8> = st.taps.as_slice()[..ctx.sources.classes().len()].to_vec();
    if !ctx.seen.insert((st.extra_used, taps_key, st.pool)) {
        return;
    }
    if !st.steps.is_empty() {
        try_finish(ctx, st);
        if ctx.stop || ctx.full {
            return;
        }
    }
    for (ei, extra) in ctx.sources.extras.iter().enumerate() {
        if st.extra_used[ei] >= extra.count {
            continue;
        }
        for (ai, act) in extra.activations.iter().enumerate() {
            let mut next_states = Vec::new();
            each_concrete_payment(
                act.price,
                st.pool,
                &st.taps,
                ctx.sources,
                false,
                &mut |price_spend, delta_plain, left_pool, next_taps, _rem_used| {
                    act.each_made(&mut |made| {
                        let mut pool_after = left_pool;
                        for (p, &m) in pool_after.iter_mut().zip(&made) {
                            *p = p.saturating_add(m);
                        }
                        let mut next_used = st.extra_used;
                        next_used[ei] += 1;
                        let mut plain = st.plain.clone();
                        plain.extend_from_slice(delta_plain);
                        let mut steps = st.steps.clone();
                        steps.push(Step {
                            extra: u8::try_from(ei).expect("fits u8"),
                            activation: u8::try_from(ai).expect("fits u8"),
                            price_spend,
                            made,
                        });
                        next_states.push(SearchState {
                            pool: pool_after,
                            taps: next_taps.clone(),
                            extra_used: next_used,
                            plain,
                            steps,
                        });
                        true
                    });
                    true
                },
            );
            for next in next_states {
                search_plans(ctx, &next);
                if ctx.stop || ctx.full {
                    return;
                }
            }
        }
    }
}

fn try_finish<L: Layout, F: FnMut(&Plan<L>) -> bool>(
    ctx: &mut PlanSearch<'_, L, F>,
    st: &SearchState<L>,
) {
    let mut candidates = Vec::new();
    each_concrete_payment(
        ctx.cost,
        st.pool,
        &st.taps,
        ctx.sources,
        true,
        &mut |final_spend, delta_plain, left_pool, final_taps, rem_used| {
            let mut plain = st.plain.clone();
            plain.extend_from_slice(delta_plain);
            candidates.push(Plan {
                spend: [0; KINDS],
                taps: final_taps.clone(),
                left: left_pool,
                remembered: rem_used,
                sequence: Some(Box::new(Sequence {
                    plain,
                    steps: st.steps.clone(),
                    final_spend,
                })),
            });
            !ctx.greedy
        },
    );
    for plan in candidates {
        if is_wasteful(
            ctx.init_pool,
            ctx.sources,
            &plan.taps,
            &st.extra_used,
            plan.remembered,
            ctx.cost,
        ) {
            continue;
        }
        if !(ctx.accepted)(&plan) {
            continue;
        }
        let outcome = ctx.sources.outcome(&plan);
        if ctx.out.iter().any(|q| ctx.sources.outcome(q) == outcome) {
            continue;
        }
        if ctx.out.push(plan).is_err() {
            ctx.full = true;
            return;
        }
        if ctx.greedy {
            ctx.stop = true;
            return;
        }
    }
}

/// Whether `cost` could still be paid from `init_pool` with one fewer extra activation or one
/// fewer plain tap than this plan used.
fn is_wasteful<L: Layout>(
    init_pool: [u8; KINDS],
    sources: &Sources<L>,
    plan_taps: &Taps<L>,
    extra_used: &[u8; CLASSES],
    rem_used: u8,
    cost: ManaCost,
) -> bool {
    let mut base_taps: Taps<L> = Table::filled(0);
    for (i, c) in sources.classes().iter().enumerate() {
        if !c.is_twin() {
            let used_here = plan_taps[i] + c.twin().map_or(0, |tw| plan_taps[usize::from(tw)]);
            base_taps[i] = c.count().saturating_sub(used_here);
        }
    }
    let mut base_extra = [0u8; CLASSES];
    for (i, e) in sources.extras.iter().enumerate() {
        base_extra[i] = e.count.saturating_sub(extra_used[i]);
    }
    let all_rem = if sources.remembered().is_empty() {
        0
    } else {
        u8::try_from((1u16 << sources.remembered().len()) - 1).expect("at most 4 remembered")
    };
    let rem_unused = all_rem & !rem_used;
    for (i, &n) in extra_used.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let mut fewer = base_extra;
        fewer[i] += 1;
        if pays_from_state(init_pool, sources, &base_taps, rem_unused, &fewer, cost) {
            return true;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        if plan_taps[i] == 0 || c.is_twin() {
            continue;
        }
        let mut fewer = base_taps.clone();
        fewer[i] += 1;
        if pays_from_state(init_pool, sources, &fewer, rem_unused, &base_extra, cost) {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn each_concrete_payment<L: Layout>(
    cost: ManaCost,
    pool: [u8; KINDS],
    taps: &Taps<L>,
    sources: &Sources<L>,
    allow_restricted: bool,
    f: &mut impl FnMut([u8; KINDS], &[(u8, u8)], [u8; KINDS], &Taps<L>, u8) -> bool,
) {
    let total = cost.total(0);
    if total == 0 {
        f([0; KINDS], &[], pool, taps, 0);
        return;
    }
    let pool_sum: u32 = pool.iter().map(|&n| u32::from(n)).sum();
    let rem_len = if allow_restricted {
        sources.remembered().len()
    } else {
        0
    };
    let mut rem_max = pool_sum + u32::try_from(rem_len).expect("fits u32");
    for (i, c) in sources.classes().iter().enumerate() {
        if !allow_restricted && c.is_restricted() {
            continue;
        }
        let avail = avail_class(sources, taps, i);
        rem_max += u32::from(avail) * u32::from(c.amount);
    }
    if rem_max < total {
        return;
    }
    let mut cur_taps = taps.clone();
    let mut delta_plain = Vec::new();
    let mut pool_spend = [0u8; KINDS];
    walk_concrete(
        cost,
        total,
        pool,
        sources,
        allow_restricted,
        0,
        0,
        0,
        &mut pool_spend,
        0,
        &mut cur_taps,
        &mut delta_plain,
        f,
    );
}

fn avail_class<L: Layout>(sources: &Sources<L>, taps: &Taps<L>, i: usize) -> u8 {
    let c = &sources.classes()[i];
    let twin_taps = c.twin().map_or(0, |tw| taps[usize::from(tw)]);
    c.count().saturating_sub(taps[i]).saturating_sub(twin_taps)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn walk_concrete<L: Layout>(
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    allow_restricted: bool,
    pos: usize,
    must: u32,
    have_max: u32,
    pool_spend: &mut [u8; KINDS],
    rem_used: u8,
    taps: &mut Taps<L>,
    delta_plain: &mut Vec<(u8, u8)>,
    f: &mut impl FnMut([u8; KINDS], &[(u8, u8)], [u8; KINDS], &Taps<L>, u8) -> bool,
) -> bool {
    let rem_len = if allow_restricted {
        sources.remembered().len()
    } else {
        0
    };
    let classes = sources.classes();
    if pos == KINDS + rem_len + classes.len() || must == total {
        if have_max < total {
            return true;
        }
        let mut combined = *pool_spend;
        for &(ci, color) in delta_plain.iter() {
            let c = &classes[usize::from(ci)];
            if !c.is_restricted() {
                combined[usize::from(color)] =
                    combined[usize::from(color)].saturating_add(c.amount);
            }
        }
        let mut units = Units::NONE;
        for (k, &n) in combined.iter().enumerate() {
            if n > 0 {
                units.add(1 << k, u16::from(n));
            }
        }
        for (ri, &(_, kind, _)) in sources.remembered().iter().enumerate() {
            if rem_used & (1 << ri) != 0 {
                units.add(1 << kind, 1);
            }
        }
        for &(ci, color) in delta_plain.iter() {
            let c = &classes[usize::from(ci)];
            if c.is_restricted() {
                units.add(1 << color, u16::from(c.amount));
            }
        }
        if !units.pays(cost) {
            return true;
        }
        let mut after_restricted = cost;
        for (ri, &(_, kind, _)) in sources.remembered().iter().enumerate() {
            if rem_used & (1 << ri) != 0 {
                super::super::pool::pay_unit(&mut after_restricted, usize::from(kind));
            }
        }
        for &(ci, color) in delta_plain.iter() {
            let c = &classes[usize::from(ci)];
            if c.is_restricted() {
                for _ in 0..c.amount {
                    super::super::pool::pay_unit(&mut after_restricted, usize::from(color));
                }
            }
        }
        let Some(spend_for_cost) = exact_concrete_spend(combined, after_restricted) else {
            return true;
        };
        let mut left = pool;
        for (l, &s) in left.iter_mut().zip(pool_spend.iter()) {
            *l -= s;
        }
        for (k, (&c, &s)) in combined.iter().zip(spend_for_cost.iter()).enumerate() {
            left[k] = left[k].saturating_add(c - s);
        }
        return f(spend_for_cost, delta_plain, left, taps, rem_used);
    }
    if pos < KINDS {
        for n in (0..=pool[pos]).rev() {
            if must + u32::from(n) > total {
                continue;
            }
            pool_spend[pos] = n;
            if !walk_concrete(
                cost,
                total,
                pool,
                sources,
                allow_restricted,
                pos + 1,
                must + u32::from(n),
                have_max + u32::from(n),
                pool_spend,
                rem_used,
                taps,
                delta_plain,
                f,
            ) {
                pool_spend[pos] = 0;
                return false;
            }
        }
        pool_spend[pos] = 0;
        return true;
    }
    if pos < KINDS + rem_len {
        let ri = pos - KINDS;
        let bit = 1u8 << ri;
        if must < total
            && !walk_concrete(
                cost,
                total,
                pool,
                sources,
                allow_restricted,
                pos + 1,
                must + 1,
                have_max + 1,
                pool_spend,
                rem_used | bit,
                taps,
                delta_plain,
                f,
            )
        {
            return false;
        }
        return walk_concrete(
            cost,
            total,
            pool,
            sources,
            allow_restricted,
            pos + 1,
            must,
            have_max,
            pool_spend,
            rem_used,
            taps,
            delta_plain,
            f,
        );
    }
    let ci = pos - KINDS - rem_len;
    let c = &classes[ci];
    if !allow_restricted && c.is_restricted() {
        return walk_concrete(
            cost,
            total,
            pool,
            sources,
            allow_restricted,
            pos + 1,
            must,
            have_max,
            pool_spend,
            rem_used,
            taps,
            delta_plain,
            f,
        );
    }
    let avail = avail_class(sources, taps, ci);
    let max_taps = u32::from(avail).min(total - must);
    let base_len = delta_plain.len();
    for n in 0..=max_taps {
        let n_u8 = u8::try_from(n).expect("fits u8");
        taps[ci] += n_u8;
        let keep_going = choose_class_colors(
            u8::try_from(ci).expect("fits u8"),
            c.kinds,
            0,
            n_u8,
            delta_plain,
            &mut |dp| {
                walk_concrete(
                    cost,
                    total,
                    pool,
                    sources,
                    allow_restricted,
                    pos + 1,
                    must + n,
                    have_max + n * u32::from(c.amount),
                    pool_spend,
                    rem_used,
                    taps,
                    dp,
                    f,
                )
            },
        );
        delta_plain.truncate(base_len);
        taps[ci] -= n_u8;
        if !keep_going {
            return false;
        }
    }
    true
}

fn choose_class_colors(
    ci: u8,
    kinds: u8,
    from: usize,
    left: u8,
    delta_plain: &mut Vec<(u8, u8)>,
    f: &mut impl FnMut(&mut Vec<(u8, u8)>) -> bool,
) -> bool {
    if left == 0 {
        return f(delta_plain);
    }
    let base = delta_plain.len();
    for k in from..KINDS {
        if kinds & (1 << k) == 0 {
            continue;
        }
        let rest_mask = kinds & !((1u8 << (k + 1)).wrapping_sub(1));
        let min_n = if rest_mask == 0 { left } else { 0 };
        for n in (min_n..=left).rev() {
            for _ in 0..n {
                delta_plain.push((ci, u8::try_from(k).expect("fits u8")));
            }
            let cont = choose_class_colors(ci, kinds, k + 1, left - n, delta_plain, f);
            delta_plain.truncate(base);
            if !cont {
                return false;
            }
        }
        return true;
    }
    true
}

fn exact_concrete_spend(avail: [u8; KINDS], cost: ManaCost) -> Option<[u8; KINDS]> {
    let mut spend = [0u8; KINDS];
    let mut left = avail;
    for (k, &need) in cost.symbols.iter().enumerate() {
        if left[k] < need {
            return None;
        }
        left[k] -= need;
        spend[k] += need;
    }
    let hybrids: Vec<(u8, u8)> = cost.hybrid.pairs().collect();
    if !pay_hybrids(&hybrids, 0, &mut left, &mut spend) {
        return None;
    }
    let mut generic = cost.generic;
    for k in 0..KINDS {
        let take = left[k].min(generic);
        left[k] -= take;
        spend[k] += take;
        generic -= take;
    }
    (generic == 0).then_some(spend)
}

fn pay_hybrids(
    hybrids: &[(u8, u8)],
    ix: usize,
    left: &mut [u8; KINDS],
    spend: &mut [u8; KINDS],
) -> bool {
    let Some(&(pair, count)) = hybrids.get(ix) else {
        return true;
    };
    let a = pair.trailing_zeros() as usize;
    let b = (pair & !(1 << a)).trailing_zeros() as usize;
    let max_a = left[a].min(count);
    for na in (0..=max_a).rev() {
        let nb = count - na;
        if nb <= left[b] {
            left[a] -= na;
            left[b] -= nb;
            spend[a] += na;
            spend[b] += nb;
            if pay_hybrids(hybrids, ix + 1, left, spend) {
                return true;
            }
            left[a] += na;
            left[b] += nb;
            spend[a] -= na;
            spend[b] -= nb;
        }
    }
    false
}
