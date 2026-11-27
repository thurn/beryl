//! The choice of plain sources and of extra sources without a price for a rich payment.
//!
//! Implements: none. Enumerates which sources a sequence plan of `mana::rich` taps.

use crate::card::ManaCost;
use crate::layout::Layout;

use super::super::sources::Sources;
use super::super::units::Units;
use super::super::{CLASSES, KINDS, Taps};
use super::Step;

/// Extra sources whose mana abilities all lack a price, such as lands under Mana Flare, that
/// make the same mana: each kind of mana one member can make, and for each member, the
/// activation that makes each. Such sources are never branched on in the search for plans:
/// a [`Walk`] chooses how many members a payment taps and for what, as it does for a plain
/// class, because what one makes does not depend on when it is activated. Members are always
/// tapped in order, so that two payments that differ only in which of two such sources they
/// tap are one.
pub(super) struct Free {
    /// The extra's index in [`Sources::extras`] and the activation for each of `outcomes`.
    members: Vec<(u8, Vec<u8>)>,
    outcomes: Vec<[u8; KINDS]>,
    /// The most mana one activation makes.
    most: u32,
}

impl Free {
    pub(super) fn of<L: Layout>(sources: &Sources<L>) -> Vec<Free> {
        let mut free: Vec<Free> = Vec::new();
        for (ei, extra) in sources.extras.iter().enumerate() {
            if !is_free(sources, ei) {
                continue;
            }
            let mut made_by: Vec<([u8; KINDS], u8)> = Vec::new();
            for (ai, act) in extra.activations.iter().enumerate() {
                act.each_made(&mut |made| {
                    if !made_by.iter().any(|&(m, _)| m == made) {
                        made_by.push((made, u8::try_from(ai).expect("fits u8")));
                    }
                    true
                });
            }
            made_by.sort_unstable();
            let consumed = extra.activations.iter().any(|a| a.consumed);
            let outcomes: Vec<[u8; KINDS]> = made_by.iter().map(|&(m, _)| m).collect();
            let acts: Vec<u8> = made_by.iter().map(|&(_, a)| a).collect();
            let member = (u8::try_from(ei).expect("fits u8"), acts);
            if let Some(group) = free.iter_mut().find(|g| {
                g.outcomes == outcomes
                    && g.members.iter().all(|&(e, _)| {
                        let other = &sources.extras[usize::from(e)];
                        other.activations.iter().any(|a| a.consumed) == consumed
                    })
            }) {
                group.members.push(member);
                continue;
            }
            let most = outcomes
                .iter()
                .map(|m| m.iter().map(|&n| u32::from(n)).sum())
                .max()
                .unwrap_or(0);
            if most > 0 {
                free.push(Free {
                    members: vec![member],
                    outcomes,
                    most,
                });
            }
        }
        free
    }
}

/// Whether every mana ability of extra `ei` of `sources` lacks a price.
pub(super) fn is_free<L: Layout>(sources: &Sources<L>, ei: usize) -> bool {
    sources.extras[ei]
        .activations
        .iter()
        .all(|a| a.price.is_zero())
}

/// What a [`Walk`] has chosen: remembered mana, plain sources, and the members of [`Free`]
/// sources, as the steps that activate them.
pub(super) struct Chosen<L: Layout> {
    pub(super) rem_used: u8,
    pub(super) taps: Taps<L>,
    /// `(class_index, color_index)` for each plain source this payment taps.
    pub(super) plain: Vec<(u8, u8)>,
    pub(super) extra_used: [u8; CLASSES],
    /// The activations of [`Free`] sources this payment makes.
    pub(super) steps: Vec<Step>,
}

/// The ways to choose, beside all of `pool`, the remembered mana, the untapped members of each
/// class and the unused members of each [`Free`] source, and the color or outcome of each, that
/// together make enough mana of the right kinds to pay `cost`, each without a source more once
/// what it chose pays.
pub(super) struct Walk<'a, L: Layout> {
    cost: ManaCost,
    total: u32,
    pool: [u8; KINDS],
    sources: &'a Sources<L>,
    free: &'a [Free],
}

impl<'a, L: Layout> Walk<'a, L> {
    pub(super) fn new(
        cost: ManaCost,
        pool: [u8; KINDS],
        sources: &'a Sources<L>,
        free: &'a [Free],
    ) -> Self {
        Walk {
            cost,
            total: cost.total(0),
            pool,
            sources,
            free,
        }
    }

    /// Calls `f` with each choice whose mana pays the cost. Where `f` returns `None`, the
    /// choice's mana pays but no order of activations does, so the walk goes on adding sources;
    /// `Some(false)` ends the walk.
    pub(super) fn each(
        &self,
        extra_used: [u8; CLASSES],
        f: &mut impl FnMut(&Chosen<L>) -> Option<bool>,
    ) {
        let mut ch = Chosen {
            rem_used: 0,
            taps: crate::layout::Table::filled(0),
            plain: Vec::new(),
            extra_used,
            steps: Vec::new(),
        };
        let pool_sum: u32 = self.pool.iter().map(|&n| u32::from(n)).sum();
        let mut most = pool_sum + u32::try_from(self.sources.remembered().len()).expect("fits");
        for (i, c) in self.sources.classes().iter().enumerate() {
            most += u32::from(avail_class(self.sources, &ch.taps, i)) * u32::from(c.amount);
        }
        for fr in self.free {
            most += u32::from(self.free_avail(fr, &ch)) * fr.most;
        }
        if most < self.total {
            return;
        }
        self.walk(0, 0, pool_sum, true, &mut ch, f);
    }

    fn free_avail(&self, fr: &Free, ch: &Chosen<L>) -> u8 {
        fr.members
            .iter()
            .map(|&(e, _)| {
                let e = usize::from(e);
                // Unused members; saturating at 0 means none left.
                self.sources.extras[e]
                    .count
                    .saturating_sub(ch.extra_used[e])
            })
            .sum()
    }

    /// Chooses position `pos` onward: the remembered mana, then the classes, then the free
    /// sources. `must` counts the sources chosen, each of which pays at least one unit of the
    /// cost in a payment that needs it, and `have_max` the most mana the pool and they make.
    /// `fresh` says the last position chose a source, so `ch` has not been offered to `f`.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        pos: usize,
        must: u32,
        have_max: u32,
        fresh: bool,
        ch: &mut Chosen<L>,
        f: &mut impl FnMut(&Chosen<L>) -> Option<bool>,
    ) -> bool {
        let rem_len = self.sources.remembered().len();
        let classes = self.sources.classes();
        let end = rem_len + classes.len() + self.free.len();
        if fresh && have_max >= self.total && self.units(ch).is_some_and(|u| u.pays(self.cost)) {
            // One more source would be one the payment does not need, unless no order of
            // activations pays with these.
            match f(ch) {
                Some(more) => return more,
                None if pos == end => return true,
                None => {}
            }
        }
        if pos == end || must == self.total {
            return true;
        }
        if pos < rem_len {
            let bit = 1u8 << pos;
            ch.rem_used |= bit;
            let more = self.walk(pos + 1, must + 1, have_max + 1, true, ch, f);
            ch.rem_used &= !bit;
            return more && self.walk(pos + 1, must, have_max, false, ch, f);
        }
        if pos < rem_len + classes.len() {
            return self.walk_class(pos, pos - rem_len, must, have_max, ch, f);
        }
        let fr = &self.free[pos - rem_len - classes.len()];
        let max_taps = u32::from(self.free_avail(fr, ch)).min(self.total - must);
        for n in 0..=max_taps {
            let n_u8 = u8::try_from(n).expect("fits u8");
            let more = choose_outcomes(self.sources, fr, 0, n_u8, ch, &mut |ch| {
                self.walk(pos + 1, must + n, have_max + n * fr.most, n > 0, ch, f)
            });
            if !more {
                return false;
            }
        }
        true
    }

    fn walk_class(
        &self,
        pos: usize,
        ci: usize,
        must: u32,
        have_max: u32,
        ch: &mut Chosen<L>,
        f: &mut impl FnMut(&Chosen<L>) -> Option<bool>,
    ) -> bool {
        let c = &self.sources.classes()[ci];
        let avail = avail_class(self.sources, &ch.taps, ci);
        let max_taps = u32::from(avail).min(self.total - must);
        let base_len = ch.plain.len();
        for n in 0..=max_taps {
            let n_u8 = u8::try_from(n).expect("fits u8");
            ch.taps[ci] += n_u8;
            let ci_u8 = u8::try_from(ci).expect("fits u8");
            let keep_going = choose_class_colors(ci_u8, c.kinds, 0, n_u8, ch, &mut |ch| {
                let more = have_max + n * u32::from(c.amount);
                self.walk(pos + 1, must + n, more, n > 0, ch, f)
            });
            ch.plain.truncate(base_len);
            ch.taps[ci] -= n_u8;
            if !keep_going {
                return false;
            }
        }
        true
    }

    /// The units of the pool and of all the mana `ch` chose: `None` where one kind is more
    /// than the layout holds, a choice the walk skips.
    fn units(&self, ch: &Chosen<L>) -> Option<Units> {
        let mut units = restricted_units(self.sources, ch);
        for (k, &n) in unrestricted(self.pool, self.sources, ch)?
            .iter()
            .enumerate()
        {
            if n > 0 {
                units.add(1 << k, u16::from(n));
            }
        }
        Some(units)
    }
}

/// `pool` and the mana `ch` chose that may pay a price, by kind: `None` where one kind is more
/// than the layout holds.
pub(super) fn unrestricted<L: Layout>(
    pool: [u8; KINDS],
    sources: &Sources<L>,
    ch: &Chosen<L>,
) -> Option<[u8; KINDS]> {
    let classes = sources.classes();
    let mut made = pool;
    for &(ci, color) in &ch.plain {
        let c = &classes[usize::from(ci)];
        if !c.is_restricted() {
            made[usize::from(color)] = made[usize::from(color)].checked_add(c.amount)?;
        }
    }
    for step in &ch.steps {
        for (k, &n) in step.made.iter().enumerate() {
            made[k] = made[k].checked_add(n)?;
        }
    }
    Some(made)
}

/// The units of the mana `ch` chose that pays only the cost: the remembered mana and the mana
/// of restricted classes.
pub(super) fn restricted_units<L: Layout>(sources: &Sources<L>, ch: &Chosen<L>) -> Units {
    let mut units = Units::NONE;
    for (ri, &(_, kind, _)) in sources.remembered().iter().enumerate() {
        if ch.rem_used & (1 << ri) != 0 {
            units.add(1 << kind, 1);
        }
    }
    for &(ci, color) in &ch.plain {
        let c = &sources.classes()[usize::from(ci)];
        if c.is_restricted() {
            units.add(1 << color, u16::from(c.amount));
        }
    }
    units
}

/// `cost` less what the mana `ch` chose that pays only the cost pays of it.
pub(super) fn after_restricted<L: Layout>(
    cost: ManaCost,
    sources: &Sources<L>,
    ch: &Chosen<L>,
) -> ManaCost {
    let mut cost = cost;
    for (ri, &(_, kind, _)) in sources.remembered().iter().enumerate() {
        if ch.rem_used & (1 << ri) != 0 {
            super::super::pool::pay_unit(&mut cost, usize::from(kind));
        }
    }
    for &(ci, color) in &ch.plain {
        let c = &sources.classes()[usize::from(ci)];
        if c.is_restricted() {
            for _ in 0..c.amount {
                super::super::pool::pay_unit(&mut cost, usize::from(color));
            }
        }
    }
    cost
}

fn avail_class<L: Layout>(sources: &Sources<L>, taps: &Taps<L>, i: usize) -> u8 {
    let c = &sources.classes()[i];
    let twin_taps = c.twin().map_or(0, |tw| taps[usize::from(tw)]);
    // Remaining untapped sources in this class; saturating at 0 means none left.
    c.count().saturating_sub(taps[i]).saturating_sub(twin_taps)
}

/// Calls `f` with each way of choosing the colors of `left` more members of class `ci`, which
/// makes `kinds`, among kinds `from` onward, pushed onto `ch.plain`.
fn choose_class_colors<L: Layout>(
    ci: u8,
    kinds: u8,
    from: usize,
    left: u8,
    ch: &mut Chosen<L>,
    f: &mut impl FnMut(&mut Chosen<L>) -> bool,
) -> bool {
    if left == 0 {
        return f(ch);
    }
    let base = ch.plain.len();
    for k in from..KINDS {
        if kinds & (1 << k) == 0 {
            continue;
        }
        let rest_mask = kinds & !((1u8 << (k + 1)).wrapping_sub(1));
        let min_n = if rest_mask == 0 { left } else { 0 };
        for n in (min_n..=left).rev() {
            for _ in 0..n {
                ch.plain.push((ci, u8::try_from(k).expect("fits u8")));
            }
            let cont = choose_class_colors(ci, kinds, k + 1, left - n, ch, f);
            ch.plain.truncate(base);
            if !cont {
                return false;
            }
        }
        return true;
    }
    true
}

/// Calls `f` with each way of choosing the outcomes of `left` more members of `fr`, among its
/// outcomes `from` onward, each tapping the first unused member, pushed onto `ch.steps`.
fn choose_outcomes<L: Layout>(
    sources: &Sources<L>,
    fr: &Free,
    from: usize,
    left: u8,
    ch: &mut Chosen<L>,
    f: &mut impl FnMut(&mut Chosen<L>) -> bool,
) -> bool {
    if left == 0 {
        return f(ch);
    }
    let Some(&made) = fr.outcomes.get(from) else {
        return true;
    };
    let min_n = if from + 1 == fr.outcomes.len() {
        left
    } else {
        0
    };
    let base = ch.steps.len();
    let used = ch.extra_used;
    for n in (min_n..=left).rev() {
        for _ in 0..n {
            let &(extra, ref acts) = fr
                .members
                .iter()
                .find(|&&(e, _)| {
                    ch.extra_used[usize::from(e)] < sources.extras[usize::from(e)].count
                })
                .expect("an unused member");
            ch.extra_used[usize::from(extra)] += 1;
            ch.steps.push(Step {
                extra,
                activation: acts[from],
                price_spend: [0; KINDS],
                made,
            });
        }
        let cont = choose_outcomes(sources, fr, from + 1, left - n, ch, f);
        ch.steps.truncate(base);
        ch.extra_used = used;
        if !cont {
            return false;
        }
    }
    true
}
