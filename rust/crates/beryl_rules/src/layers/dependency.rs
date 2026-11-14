//! The order of the effects of one layer when one depends on another ([CR 613.8]):
//! "Within a layer or sublayer, determining which order effects are applied in is sometimes
//! done using a dependency system. If a dependency exists, it will override the timestamp
//! system."
//!
//! Whether one effect depends on another is found by trying it, never from a list of cards
//! or kinds: the other is applied to the layer results, the first's existence, set and change
//! are compared before and after, and the results are put back. Each effect declares what it
//! reads and what it writes among the characteristics, and a pair whose sets do not meet is
//! never tried, so a layer whose effects read nothing the others write applies in timestamp
//! order with no trial.
//!
//! The effects of characteristic-defining abilities are compared only with each other
//! ([CR 613.8a], (c)): the cards' only ones define power and toughness, in layer 7a, where
//! no other effect applies.
//!
//! [CR 613.8]: https://yawgatog.com/resources/magic-rules/#R6138
//! [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a

use beryl_kernel::derive::Rederive;

use super::{Apply, Begun, Target, apply, apply_one, members, position};
use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::layout::{Layout, Queue, Table};
use crate::state::{BATTLEFIELD, Characteristics, Derived, LayerChange};
use crate::static_ability;
use crate::table::GameTable;

/// The static abilities that began in an earlier layer, with the sets they began on
/// ([`Begun`]).
type Carried<L> = Option<<L as Layout>::Queue<Begun<L>, 32>>;

/// [`Carried`], lent to read.
type Read<'b, L> = Option<&'b <L as Layout>::Queue<Begun<L>, 32>>;

/// Whether some effect of `segment`, the effects of one layer in timestamp order, reads what
/// another writes, so that it may depend on it: the test that keeps every other layer free
/// of trials. A layer whose effects write nothing any effect reads, as the layers of power
/// and toughness, is answered with no look at a card; any other with a few ANDs for each
/// pair.
pub(super) fn may_depend<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    segment: &[Apply<L>],
) -> bool {
    let written = (segment.iter()).fold(Characteristics::NONE, |w, a| w.union(a.change.writes()));
    if !written.intersects(Characteristics::READABLE) {
        return false;
    }
    segment.iter().enumerate().any(|(i, a)| {
        let reads = reads(s, t, a);
        reads != Characteristics::NONE
            && segment
                .iter()
                .enumerate()
                .any(|(j, b)| i != j && !same_effect(a, b) && reads.intersects(b.change.writes()))
    })
}

/// Applies `segment`, the effects of one layer, each after those it depends on
/// ([CR 613.8b]): "An effect dependent on one or more other effects waits to apply until
/// just after all of those effects have been applied. If multiple dependent effects would
/// apply simultaneously in this way, they're applied in timestamp order relative to each
/// other. If several dependent effects form a dependency loop, then this rule is ignored and
/// the effects in the dependency loop are applied in timestamp order." After each, the
/// dependencies of those left are found again ([CR 613.8c]): "After each effect is applied,
/// the order of remaining effects is reevaluated".
///
/// `trials` counts the trials of one effect against another ([`depends`]). Nothing here
/// allocates in a fixed layout: the engine's loop never does.
///
/// Out of line: only a layer with effects that may depend on each other comes here.
///
/// [CR 613.8b]: https://yawgatog.com/resources/magic-rules/#R6138b
/// [CR 613.8c]: https://yawgatog.com/resources/magic-rules/#R6138c
#[inline(never)]
pub(super) fn apply_ordered<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    segment: &[Apply<L>],
    begun: &mut Carried<L>,
    trials: &mut u64,
) -> Result<(), Full> {
    let mut applied = Ok(());
    // The effects still to apply, by their place in `segment`, which is timestamp order. A
    // segment is part of the recompute's list, which has as many places as this.
    let mut pending: L::Queue<u8, 192> = Default::default();
    for i in 0..segment.len() {
        pending.push_bounded(i as u8);
    }
    while !pending.is_empty() {
        // The earliest that depends on none left, or, with every one waiting, the earliest
        // in a loop of dependencies.
        let read = begun.as_ref();
        let waits = |s: &mut Rederive<'_, L>, k: u8, trials: &mut u64| {
            pending.iter().any(|&p| {
                let (a, b) = (&segment[usize::from(k)], &segment[usize::from(p)]);
                depends(s, t, a, b, read, trials)
            })
        };
        let mut next = None;
        for (i, &k) in pending.iter().enumerate() {
            if !waits(s, k, trials) {
                next = Some(i);
                break;
            }
        }
        let next = next
            .or_else(|| {
                (0..pending.len())
                    .find(|&i| in_loop(s, t, segment, pending.as_slice(), i, read, trials))
            })
            .unwrap_or(0);
        let a = segment[usize::from(pending.remove(next))];
        applied = applied.and(apply_one(s, t, &a, begun));
    }
    applied
}

/// Whether the effect at `k` of `pending` (places in `segment`) reaches itself through what
/// each effect left depends on: a search over the effects, each visited once.
fn in_loop<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    segment: &[Apply<L>],
    pending: &[u8],
    k: usize,
    begun: Read<'_, L>,
    trials: &mut u64,
) -> bool {
    let start = pending[k];
    // A bit for each place of `segment` already met.
    let mut seen = [0u64; 4];
    let mut stack: L::Queue<u8, 192> = Default::default();
    stack.push_bounded(start);
    while let Some(j) = stack.pop() {
        for &p in pending {
            if !depends(
                s,
                t,
                &segment[usize::from(j)],
                &segment[usize::from(p)],
                begun,
                trials,
            ) {
                continue;
            }
            if p == start {
                return true;
            }
            let (word, bit) = (usize::from(p) / 64, 1u64 << (p % 64));
            if seen[word] & bit == 0 {
                seen[word] |= bit;
                // Each place is pushed once, and there are no more than the stack holds.
                stack.push_bounded(p);
            }
        }
    }
    false
}

/// Whether `a` depends on `b` ([CR 613.8a]): "applying the other would change the text or
/// the existence of the first effect, what it applies to, or what it does to any of the
/// things it applies to". Tried only when `b` writes what `a` reads: `b` is applied to the
/// layer results, `a`'s existence, set and change are compared with what they were, and the
/// results are put back. Each trial counts in `trials`.
///
/// [CR 613.8a]: https://yawgatog.com/resources/magic-rules/#R6138a
fn depends<L: Layout>(
    s: &mut Rederive<'_, L>,
    t: &GameTable,
    a: &Apply<L>,
    b: &Apply<L>,
    begun: Read<'_, L>,
    trials: &mut u64,
) -> bool {
    if a == b || same_effect(a, b) || !reads(s, t, a).intersects(b.change.writes()) {
        return false;
    }
    *trials += 1;
    let before = what_it_does(s, t, a, begun);
    let saved: L::Table<Derived<L>, BATTLEFIELD> = {
        let rows = s.rows();
        Table::from_fn(|i| rows.get(i).copied().unwrap_or_default())
    };
    if let Some(members) = members(s, t, b, begun) {
        let you = controller(s, b);
        // A removal past a mask's width changes the trial no more than the game, which
        // stalls once the effect applies for real.
        let _ = apply::apply_to_members(s, t, members, b.source, b.change, you);
    }
    let after = what_it_does(s, t, a, begun);
    let len = s.rows().len();
    s.results().copy_from_slice(&saved.as_slice()[..len]);
    before != after
}

/// What `a` does now: the permanents it applies to, none if it does not exist, and the
/// change it makes to each.
fn what_it_does<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    a: &Apply<L>,
    begun: Read<'_, L>,
) -> (Option<Bitboard<L::Slot>>, LayerChange) {
    let members = members(s, t, a, begun);
    let change = match a.change {
        LayerChange::Read { .. } if members.is_some() => {
            apply::settle(s, t, a.source, a.change, controller(s, a))
        }
        change => change,
    };
    (members, change)
}

/// The controller of the permanent with the static ability `a` is part of, as the layers so
/// far left it; none for a resolved effect.
fn controller<L: Layout>(s: &Rederive<'_, L>, a: &Apply<L>) -> Option<crate::ids::Seat> {
    a.from
        .map(|(from, _)| s.rows()[position::<L>(from)].controller)
}

/// Whether `a` and `b` are parts of one static ability's effect, which apply in the order
/// the card says them ([CR 613.6]) and never wait for each other.
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
fn same_effect<L: Layout>(a: &Apply<L>, b: &Apply<L>) -> bool {
    match (a.from, b.from) {
        (Some((x, _)), Some((y, _))) => x == y && a.order == b.order,
        _ => false,
    }
}

/// The characteristics finding `a`'s existence, set and change reads of the layer results.
/// A static ability exists while its permanent has not lost it, which only a removal
/// changes; one that finds its set as it applies reads what its filter and "as long as"
/// read; one whose amount the layers read reads what the amount counts.
fn reads<L: Layout>(s: &Rederive<'_, L>, t: &GameTable, a: &Apply<L>) -> Characteristics {
    let Some((from, first)) = a.from else {
        return Characteristics::NONE;
    };
    let ability = super::static_ability_of(s, t, from, a);
    let Some(spec) = ability.layer() else {
        unreachable!("the recompute lists the static abilities that have a layer effect");
    };
    let mut reads = match a.change {
        LayerChange::Read { part, .. } => apply::amount_reads(spec, part),
        _ => Characteristics::NONE,
    };
    if first {
        reads = reads.union(Characteristics::REMOVES_ABILITIES);
        if let Target::Static(_) = a.target {
            if let beryl_card::Objects::All { filter, .. }
            | beryl_card::Objects::Other(&beryl_card::Objects::All { filter, .. }) =
                spec.affected
            {
                reads = reads.union(apply::filter_reads(filter));
            }
            // An "as long as" that reads the results asks what filters ask.
            if spec
                .condition
                .as_ref()
                .is_some_and(static_ability::reads_results)
            {
                reads = reads.union(Characteristics::FILTERED);
            }
        }
    }
    reads
}
