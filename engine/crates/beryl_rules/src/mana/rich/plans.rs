//! Sequence plan search for priced and multi-term mana abilities.
//!
//! A sequence activates its priced mana abilities in some order, each paying its price from the
//! pool as it then stands. The other sources it taps, the plain classes and the extras whose
//! abilities all lack a price ([`Free`]), make the same mana whenever they are activated, so a
//! sequence activates them all first. The search therefore branches only on which priced
//! activations a payment makes. For each choice, one [`Walk`] chooses the other sources for the
//! cost and the prices together, and [`PlanSearch::order`] finds the orders of the priced
//! activations whose prices the pool pays.
//!
//! Implements: none. Enumerates non-wasteful payment sequences for `mana::rich`.

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
use super::super::{CLASSES, KINDS, Taps};
use super::walk::{Chosen, Free, Walk, after_restricted, is_free, restricted_units, unrestricted};
use super::{Sequence, Step, each_pool_payment, pays_from_state, pays_with_sources};

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
    let mut options = Vec::new();
    for (ei, extra) in sources.extras.iter().enumerate() {
        if is_free(sources, ei) {
            continue;
        }
        for (ai, act) in extra.activations.iter().enumerate() {
            act.each_made(&mut |made| {
                options.push(Priced {
                    extra: u8::try_from(ei).expect("fits u8"),
                    activation: u8::try_from(ai).expect("fits u8"),
                    price: act.price,
                    made,
                });
                true
            })?;
        }
    }
    let free = Free::of(sources)?;
    let mut ctx = PlanSearch {
        init_pool: s.player(player).mana().amounts_u8(),
        cost,
        greedy,
        sources,
        options,
        accepted,
        out,
        full: false,
        stop: false,
    };
    ctx.search(&free, 0, &mut [0; CLASSES], &mut Vec::new());
    if ctx.full { Err(Full) } else { Ok(()) }
}

/// An activation of a priced extra a plan may make, with what it makes; the payment of its
/// price is chosen once the order of the activations is.
#[derive(Clone, Copy)]
struct Priced {
    extra: u8,
    activation: u8,
    price: ManaCost,
    made: [u8; KINDS],
}

struct PlanSearch<'a, L: Layout, F: FnMut(&Plan<L>) -> bool> {
    init_pool: [u8; KINDS],
    cost: ManaCost,
    greedy: bool,
    sources: &'a Sources<L>,
    /// Every activation of every extra with a priced ability, for each thing it can make.
    options: Vec<Priced>,
    accepted: &'a mut F,
    out: &'a mut L::Queue<Plan<L>, 64>,
    full: bool,
    stop: bool,
}

impl<L: Layout, F: FnMut(&Plan<L>) -> bool> PlanSearch<'_, L, F> {
    fn done(&self) -> bool {
        self.stop || self.full
    }

    /// Plans for the priced activations `chosen`, indices into `options` in order, and for
    /// each way of adding more of `options` from `from` onward. `used` counts the members of
    /// each extra `chosen` activates.
    fn search(
        &mut self,
        free: &[Free],
        from: usize,
        used: &mut [u8; CLASSES],
        chosen: &mut Vec<usize>,
    ) {
        self.finish(free, used, chosen);
        for i in from..self.options.len() {
            if self.done() {
                return;
            }
            let ei = usize::from(self.options[i].extra);
            if used[ei] >= self.sources.extras[ei].count {
                continue;
            }
            used[ei] += 1;
            chosen.push(i);
            // Activations that are already wasteful lead only to wasteful plans: more sources
            // never make one less so.
            let taps: Taps<L> = Table::filled(0);
            if !is_wasteful(self.init_pool, self.sources, &taps, used, 0, self.cost) {
                self.search(free, i, used, chosen);
            }
            chosen.pop();
            used[ei] -= 1;
        }
    }

    /// Offers the plans that make the priced activations `chosen` and tap other sources.
    fn finish(&mut self, free: &[Free], used: &[u8; CLASSES], chosen: &[usize]) {
        let mut need = self.cost;
        let mut pool = self.init_pool;
        for &i in chosen {
            let option = self.options[i];
            need = need.plus(option.price);
            for (p, &m) in pool.iter_mut().zip(&option.made) {
                // More mana of one kind than the layout holds: the game goes on in a larger one.
                let Some(sum) = p.checked_add(m) else {
                    self.full = true;
                    return;
                };
                *p = sum;
            }
        }
        let walk = Walk::new(need, pool, self.sources, free);
        walk.each(*used, &mut |ch| {
            if chosen.is_empty() && ch.steps.is_empty() {
                // A payment with no extra activation is the plain path's plan.
                return Some(true);
            }
            let mut order = chosen.to_vec();
            let mut paid = false;
            loop {
                paid |= self.order(ch, &order);
                if self.done() || !next_permutation(&mut order) {
                    break;
                }
            }
            paid.then_some(!self.done())
        });
    }

    /// Offers the plans that tap what `ch` chose and then make the priced activations `order`
    /// in that order, for each way of paying their prices from the pool; whether there is one.
    fn order(&mut self, ch: &Chosen<L>, order: &[usize]) -> bool {
        let Some(pool) = unrestricted(self.init_pool, self.sources, ch) else {
            self.full = true;
            return false;
        };
        let mut steps = ch.steps.clone();
        let mut paid = false;
        self.pay_prices(ch, order, pool, &mut steps, &mut paid);
        paid
    }

    fn pay_prices(
        &mut self,
        ch: &Chosen<L>,
        order: &[usize],
        pool: [u8; KINDS],
        steps: &mut Vec<Step>,
        paid: &mut bool,
    ) {
        let Some((&first, rest)) = order.split_first() else {
            self.pay_cost(ch, pool, steps, paid);
            return;
        };
        let option = self.options[first];
        each_pool_payment(pool, option.price, &mut |price_spend, left| {
            let mut after = left;
            for (p, &m) in after.iter_mut().zip(&option.made) {
                let Some(sum) = p.checked_add(m) else {
                    self.full = true;
                    return false;
                };
                *p = sum;
            }
            steps.push(Step {
                extra: option.extra,
                activation: option.activation,
                price_spend,
                made: option.made,
            });
            self.pay_prices(ch, rest, after, steps, paid);
            steps.pop();
            !self.done()
        });
    }

    fn pay_cost(&mut self, ch: &Chosen<L>, pool: [u8; KINDS], steps: &[Step], paid: &mut bool) {
        let mut units = restricted_units(self.sources, ch);
        for (k, &n) in pool.iter().enumerate() {
            units.add(1 << k, u16::from(n));
        }
        if !units.pays(self.cost) {
            return;
        }
        let rest = after_restricted(self.cost, self.sources, ch);
        let Some(final_spend) = exact_concrete_spend(pool, rest) else {
            return;
        };
        *paid = true;
        let mut left = pool;
        for (l, &s) in left.iter_mut().zip(&final_spend) {
            *l -= s;
        }
        let found = Plan {
            spend: [0; KINDS],
            taps: ch.taps.clone(),
            left,
            remembered: ch.rem_used,
            sequence: Some(Box::new(Sequence {
                plain: ch.plain.clone(),
                steps: steps.to_vec(),
                final_spend,
            })),
        };
        // The walk began from the members the priced activations use.
        self.offer(found, &ch.extra_used);
    }

    /// Keeps `plan`, which activates `extra_used` of each extra, unless it is wasteful, a plan
    /// with its outcome taps no more sources, or `accepted` refuses it.
    fn offer(&mut self, plan: Plan<L>, extra_used: &[u8; CLASSES]) {
        // Whether the plan is wasteful costs far more to ask than the rest, so it is asked last.
        let outcome = self.sources.outcome(&plan);
        let same = self
            .out
            .iter()
            .position(|q| self.sources.outcome(q) == outcome);
        if same.is_some_and(|pos| plan_taps(&plan) >= plan_taps(&self.out.as_slice()[pos])) {
            return;
        }
        let wasteful = is_wasteful(
            self.init_pool,
            self.sources,
            &plan.taps,
            extra_used,
            plan.remembered,
            self.cost,
        );
        if wasteful || !(self.accepted)(&plan) {
            return;
        }
        if let Some(pos) = same {
            self.out.as_mut_slice()[pos] = plan;
        } else if self.out.push(plan).is_err() {
            self.full = true;
        } else if self.greedy {
            self.stop = true;
        }
    }
}

/// Rearranges `order` into the next greater permutation, as C++'s `next_permutation`: false,
/// leaving it sorted, after the greatest.
fn next_permutation(order: &mut [usize]) -> bool {
    let Some(i) = order.windows(2).rposition(|w| w[0] < w[1]) else {
        order.reverse();
        return false;
    };
    let j = order
        .iter()
        .rposition(|&x| x > order[i])
        .expect("a greater element");
    order.swap(i, j);
    order[i + 1..].reverse();
    true
}

/// Total number of sources a plan taps (plain sources plus rich steps).
fn plan_taps<L: Layout>(plan: &Plan<L>) -> usize {
    let base: usize = plan.taps.as_slice().iter().map(|&n| usize::from(n)).sum();
    let extra = plan.sequence.as_deref().map_or(0, |seq| seq.steps.len());
    base + extra
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
            // Remaining untapped sources in this class; saturating at 0 means none left.
            base_taps[i] = c.count().saturating_sub(used_here);
        }
    }
    let mut base_extra = [0u8; CLASSES];
    for (i, e) in sources.extras.iter().enumerate() {
        // Remaining unused activations in this extra class; saturating at 0 means none left.
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
