//! The reference for payments that activate mana abilities in sequence: every order of the
//! mana abilities a player could activate, each paying its own price from a pool it simulates,
//! then the cost matched symbol by symbol as [`super::reference_pays`] matches it. It reads
//! each mana ability's cost and product off the cards ([`State::abilities`]) and shares no
//! code with the solver (`mana::rich`, `mana::sources`), so a disagreement between the two is
//! a bug in one of them.
//!
//! Implements: none. Checks the mana solver's sequences of mana abilities by brute force.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::card::{AbilityKind, Amount, CostAction, CostPart, Keywords, ManaCost, Timing, Types};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;

use super::{UNITS, controls, reference_pays};

/// The most mana abilities with a price a position may have for the brute force to try every
/// order of them: past it, the position is skipped and counted in [`SKIPPED`].
const MOST_PRICED: usize = 10;

/// How many positions [`reference_sequences`] skipped for having more than [`MOST_PRICED`]
/// mana abilities with a price. Only written: no game reads it, so it changes no outcome.
pub(crate) static SKIPPED: AtomicU64 = AtomicU64::new(0);

/// One mana ability a permanent may be tapped for: its price, the mana part of its cost, and
/// each set of units of mana it may make, one for each way of choosing the kind of a term
/// whose units are all one kind ([`crate::card::Term::uniform`]). Each unit is the set of
/// kinds it may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Activation {
    pub(crate) price: ManaCost,
    pub(crate) makes: Vec<Vec<u8>>,
}

impl Activation {
    /// An activation of `price` whose product is `terms`, each its kinds, how many units it
    /// makes and whether all its units are one kind.
    pub(crate) fn of(price: ManaCost, terms: &[(u8, u8, bool)]) -> Activation {
        let mut makes = vec![Vec::new()];
        for &(kinds, amount, uniform) in terms {
            let choices: Vec<u8> = if uniform {
                (0..6)
                    .map(|k| 1u8 << k)
                    .filter(|k| kinds & k != 0)
                    .collect()
            } else {
                vec![kinds]
            };
            makes = makes
                .iter()
                .flat_map(|made| {
                    choices.iter().map(move |&kind| {
                        let mut more = made.clone();
                        more.extend(std::iter::repeat_n(kind, usize::from(amount)));
                        more
                    })
                })
                .collect();
        }
        Activation { price, makes }
    }

    fn is_priced(&self) -> bool {
        !self.price.is_zero()
    }
}

/// Whether `player` can pay `cost` (X announced) for no spell by activating some of their mana
/// abilities in some order, each paying its price from the mana then in their pool, and paying
/// the cost from what is left (see [CR 605.3a]): "A player may activate an activated mana
/// ability ... whenever they are casting a spell or activating an ability that requires a mana
/// payment". `None` where a mana ability is one this
/// reference does not read yet (a cost part besides {T} and sacrificing its source, or an
/// amount that counts), where the units outgrow [`UNITS`], or where more than [`MOST_PRICED`]
/// mana abilities have a price, which [`SKIPPED`] counts.
///
/// [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a
pub(crate) fn reference_sequences<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
) -> Option<bool> {
    let mut pool = Vec::new();
    for (k, &n) in s.player(player).mana().amounts().iter().enumerate() {
        pool.extend(std::iter::repeat_n(1u8 << k, usize::from(n)));
    }
    sequences_pay(&pool, &activations(s, t, player)?, cost)
}

/// The mana abilities each untapped permanent `player` controls could be tapped for now, read
/// from the abilities it has, with the intrinsic ones of its basic land types (see CR 305.6).
/// Restricted mana is left out, as it pays for no cost of no spell (see CR 106.6), and so is a
/// mana ability activated only as an instant, which no payment activates.
///
/// See [CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026): "A creature's activated
/// ability with the tap symbol or the untap symbol in its activation cost can't be activated
/// unless the creature has been under its controller's control continuously since their most
/// recent turn began."
fn activations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Option<Vec<Vec<Activation>>> {
    let mut permanents = Vec::new();
    for &slot in s.battlefield() {
        let (d, o) = (s.derived(slot), s.object(slot));
        if d.controller != player
            || o.is_tapped()
            || (d.types.intersects(Types::CREATURE)
                && o.is_summoning_sick()
                && !d.keywords.contains(Keywords::HASTE))
        {
            continue;
        }
        let intrinsic = s.intrinsic_mana(t, slot);
        let mut options: Vec<Activation> = (0..6)
            .map(|k| 1u8 << k)
            .filter(|k| intrinsic & k != 0)
            .map(|k| Activation::of(ManaCost::NONE, &[(k, 1, false)]))
            .collect();
        let words = s.words_of(slot);
        for (_, a) in s.abilities(t, slot) {
            if a.kind() != AbilityKind::Mana
                || a.timing() == Timing::AsInstant
                || a.spend().is_some()
                || !controls(s, t, player, a, &words)
            {
                continue;
            }
            options.push(activation_on(s, t, slot, a)?);
        }
        if !options.is_empty() {
            permanents.push(options);
        }
    }
    Some(permanents)
}

/// The activation of a mana ability on `slot`, read from its cost and product: `None` for a cost
/// with a part besides {T} and sacrificing its source.
fn activation_on<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    a: &crate::card::Ability,
) -> Option<Activation> {
    let tap_only = a.cost().does(CostAction::Tap)
        && a.cost().parts().iter().all(|part| {
            matches!(
                part,
                CostPart::This(CostAction::Tap | CostAction::Sacrifice)
            )
        });
    if !tap_only {
        return None;
    }
    let mut terms = Vec::new();
    for term in a.product() {
        let n = if let Amount::Fixed(n) = term.amount {
            n
        } else {
            crate::interpret::amount_of(s, t, slot, term.amount)
        };
        if n > 0 {
            terms.push((term.kinds, n, term.uniform));
        }
    }
    Some(Activation::of(a.cost().mana, &terms))
}

#[cfg(test)]
fn activation(a: &crate::card::Ability) -> Option<Activation> {
    let tap_only = a.cost().does(CostAction::Tap)
        && a.cost().parts().iter().all(|part| {
            matches!(
                part,
                CostPart::This(CostAction::Tap | CostAction::Sacrifice)
            )
        });
    if !tap_only {
        return None;
    }
    let mut terms = Vec::new();
    for term in a.product() {
        let Amount::Fixed(n) = term.amount else {
            return None;
        };
        terms.push((term.kinds, n, term.uniform));
    }
    Some(Activation::of(a.cost().mana, &terms))
}

/// Whether `cost` can be paid from the units of `pool` and those that the permanents'
/// activations make, each permanent tapped for at most one of its activations, in an order in
/// which each activation's price is paid from the units then in the pool: every order and every
/// way of paying each price, tried. `None` as [`reference_sequences`] says.
pub(crate) fn sequences_pay(
    pool: &[u8],
    permanents: &[Vec<Activation>],
    cost: ManaCost,
) -> Option<bool> {
    let priced = permanents
        .iter()
        .flatten()
        .filter(|a| a.is_priced())
        .count();
    if priced > MOST_PRICED {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    // A permanent with no priced activation is tapped first, for one of them: more mana never
    // makes a cost unpayable, and what it makes may pay a price. Where each of its
    // activations makes one unit, that is one unit of any of their kinds.
    let (flexible, plain): (Vec<_>, Vec<_>) = permanents
        .iter()
        .partition(|options| options.iter().any(Activation::is_priced));
    let mut starts: Vec<Vec<u8>> = vec![pool.to_vec()];
    for options in plain {
        let makes: Vec<&Vec<u8>> = options.iter().flat_map(|a| &a.makes).collect();
        let one_unit = makes.iter().all(|made| made.len() == 1);
        let ways: Vec<Vec<u8>> = if one_unit {
            vec![vec![makes.iter().fold(0, |kinds, made| kinds | made[0])]]
        } else {
            makes.into_iter().cloned().collect()
        };
        starts = starts
            .iter()
            .flat_map(|start| {
                ways.iter().map(move |made| {
                    let mut more = start.clone();
                    more.extend(made);
                    more
                })
            })
            .collect();
    }
    let flexible: Vec<&Vec<Activation>> = flexible.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut unknown = false;
    for start in starts {
        match search(start, 0, &flexible, cost, &mut seen) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unknown = true,
        }
    }
    (!unknown).then_some(false)
}

/// [`sequences_pay`] from `pool` once the permanents in `used`, a bit per place in `flexible`,
/// were tapped: whether the cost is paid now, or after tapping one more. `seen` holds the
/// positions already tried.
fn search(
    mut pool: Vec<u8>,
    used: u16,
    flexible: &[&Vec<Activation>],
    cost: ManaCost,
    seen: &mut BTreeSet<(u16, Vec<u8>)>,
) -> Option<bool> {
    if pool.len() > UNITS {
        return None;
    }
    if reference_pays(&pool, cost) {
        return Some(true);
    }
    pool.sort_unstable();
    if !seen.insert((used, pool.clone())) {
        return Some(false);
    }
    let mut unknown = false;
    for (i, options) in flexible.iter().enumerate() {
        if used & (1 << i) != 0 {
            continue;
        }
        for activation in *options {
            for left in payments(&pool, activation.price) {
                for made in &activation.makes {
                    let mut next = left.clone();
                    next.extend(made);
                    match search(next, used | (1 << i), flexible, cost, seen) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => unknown = true,
                    }
                }
            }
        }
    }
    (!unknown).then_some(false)
}

/// What is left of `pool`, a sorted list of units, after each way of paying `price` from it:
/// every choice of as many units as the price takes that pays it, each choice of how many units
/// of each set of kinds once.
fn payments(pool: &[u8], price: ManaCost) -> Vec<Vec<u8>> {
    let total = usize::try_from(price.total(0)).expect("a price fits a usize");
    if total == 0 {
        return vec![pool.to_vec()];
    }
    let mut kinds: Vec<(u8, usize)> = Vec::new();
    for &unit in pool {
        match kinds.last_mut() {
            Some((last, n)) if *last == unit => *n += 1,
            _ => kinds.push((unit, 1)),
        }
    }
    let mut out = Vec::new();
    let mut counts = vec![0; kinds.len()];
    choose(&kinds, 0, total, &mut counts, &mut |counts| {
        let spent: Vec<u8> = (kinds.iter().zip(counts))
            .flat_map(|(&(unit, _), &n)| std::iter::repeat_n(unit, n))
            .collect();
        if reference_pays(&spent, price) {
            let left = (kinds.iter().zip(counts))
                .flat_map(|(&(unit, have), &n)| std::iter::repeat_n(unit, have - n))
                .collect();
            out.push(left);
        }
    });
    out
}

/// Calls `each` with every way of taking `left` units from the sets of kinds of `kinds` from
/// the one at `at` on, as a count for each set.
fn choose(
    kinds: &[(u8, usize)],
    at: usize,
    left: usize,
    counts: &mut Vec<usize>,
    each: &mut dyn FnMut(&[usize]),
) {
    if at == kinds.len() {
        if left == 0 {
            each(counts);
        }
        return;
    }
    for n in 0..=left.min(kinds[at].1) {
        counts[at] = n;
        choose(kinds, at + 1, left - n, counts, each);
    }
    counts[at] = 0;
}

#[cfg(test)]
mod tests {
    use super::{Activation, activation, sequences_pay};
    use crate::card::{Ability, Cost, CostPart, ManaColor, ManaCost, ManaSet, mana};
    use beryl_card::Term;

    const W: u8 = 1;
    const U: u8 = 2;

    /// "{1}, {T}: Add {C}{C}.", whose product is two terms.
    static STONE: Ability = Ability::mana_of(
        Cost::of(mana("{1}"), &[CostPart::TAP]),
        &[
            Term::one(ManaColor::Colorless),
            Term::one(ManaColor::Colorless),
        ],
    );
    static PLAINS: Ability = Ability::tap_for_mana(ManaSet::one(ManaColor::White));

    /// "{1}, {T}: Add {C}{C}.", as two terms of one mana.
    fn mind_stone_like() -> Vec<Activation> {
        vec![activation(&STONE).expect("read")]
    }

    fn plains() -> Vec<Activation> {
        vec![activation(&PLAINS).expect("read")]
    }

    /// A source that costs {1} to make two mana cannot pay for itself: its own mana is made
    /// after its price is paid.
    #[test]
    fn a_priced_source_cannot_pay_its_own_price() {
        let alone = [mind_stone_like()];
        assert_eq!(
            sequences_pay(&[], &alone, ManaCost::generic(1)),
            Some(false)
        );
        assert_eq!(
            sequences_pay(&[], &alone, ManaCost::generic(2)),
            Some(false)
        );
        // One floating mana pays its price: two mana, never three.
        assert_eq!(
            sequences_pay(&[U], &alone, ManaCost::generic(2)),
            Some(true)
        );
        assert_eq!(
            sequences_pay(&[U], &alone, ManaCost::generic(3)),
            Some(false)
        );
    }

    /// Two of them and one land make three: the land pays the first price, and one of its two
    /// mana pays the second's.
    #[test]
    fn two_priced_sources_and_a_land_make_three() {
        let board = [mind_stone_like(), mind_stone_like(), plains()];
        assert_eq!(sequences_pay(&[], &board, ManaCost::generic(3)), Some(true));
        assert_eq!(
            sequences_pay(&[], &board, ManaCost::generic(4)),
            Some(false)
        );
        // The Plains' {W} pays a price or the cost, never both: no {W} is left once the
        // stones are tapped, and without them it is the only mana.
        assert_eq!(sequences_pay(&[], &board, mana("{W}")), Some(true));
        assert_eq!(sequences_pay(&[], &board, mana("{1}{W}")), Some(false));
        assert_eq!(sequences_pay(&[], &board, mana("{C}{C}{C}")), Some(true));
    }

    /// Units of a term chosen apart may each be any of its kinds; a term of one kind for all
    /// its units makes all of them that kind.
    #[test]
    fn a_term_chooses_its_kinds_as_it_says() {
        let apart = [vec![Activation::of(ManaCost::NONE, &[(W | U, 2, false)])]];
        assert_eq!(sequences_pay(&[], &apart, mana("{W}{U}")), Some(true));
        let together = [vec![Activation::of(ManaCost::NONE, &[(W | U, 2, true)])]];
        assert_eq!(sequences_pay(&[], &together, mana("{W}{U}")), Some(false));
        assert_eq!(sequences_pay(&[], &together, mana("{U}{U}")), Some(true));
    }
}
