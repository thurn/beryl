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
use crate::filter::Evaluate;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Payload, State};
use crate::table::GameTable;
use beryl_card::{Added, Behavior, ManaChange, Tapped, Words};

use super::{UNITS, controls, reference_pays, spendable};
use crate::mana::Paying;

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
    reference_sequences_for(s, t, player, cost, Paying::NOTHING)
}

/// [`reference_sequences`] for what `paying` is for, as a spell: the restricted mana that its
/// restriction accepts pays too, both the mana that remembers in the pool and the mana of a
/// source whose mana ability makes restricted mana (see [CR 106.6]): "Some spells or abilities
/// that produce mana restrict how that mana can be spent".
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
pub(crate) fn reference_sequences_for<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    paying: Paying<L>,
) -> Option<bool> {
    let mut pool = Vec::new();
    for (k, &n) in s.player(player).mana().amounts_u8().iter().enumerate() {
        pool.extend(std::iter::repeat_n(1u8 << k, usize::from(n)));
    }
    if paying.reads_restrictions(t) {
        for mana in s.player(player).remembered_mana() {
            if paying.accepts(t, mana.restriction, mana.chosen_type) {
                pool.push(crate::card::ManaSet::one(mana.color).colors);
            }
        }
    }
    sequences_pay(&pool, &activations(s, t, player, paying)?, cost)
}

/// The mana abilities each untapped permanent `player` controls could be tapped for now, read
/// from the abilities it has, with the intrinsic ones of its basic land types (see CR 305.6).
/// Restricted mana is left out unless `paying` accepts it (see CR 106.6), and so is a mana
/// ability activated only as an instant, which no payment activates.
///
/// See [CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026): "A creature's activated
/// ability with the tap symbol or the untap symbol in its activation cost can't be activated
/// unless the creature has been under its controller's control continuously since their most
/// recent turn began."
fn activations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    paying: Paying<L>,
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
        let (extra_terms, type_produced, multiplier) = live_mana_changes(s, t, slot)?;
        let intrinsic = s.intrinsic_mana(t, slot);
        let mut options: Vec<Activation> = (0..6)
            .map(|k| 1u8 << k)
            .filter(|k| intrinsic & k != 0)
            .map(|k| {
                let mut terms =
                    Vec::with_capacity(1 + extra_terms.len() + usize::from(type_produced > 0));
                terms.push((k, multiplier, false));
                terms.extend_from_slice(&extra_terms);
                if type_produced > 0 {
                    terms.push((k, type_produced, false));
                }
                Activation::of(ManaCost::NONE, &terms)
            })
            .collect();
        let words = s.words_of(slot);
        for (_, a) in s.abilities(t, slot) {
            if a.kind() != AbilityKind::Mana
                || a.timing() == Timing::AsInstant
                || !spendable(s, t, paying, slot, a)
                || !controls(s, t, player, a, &words)
            {
                continue;
            }
            options.push(activation_on(
                s,
                t,
                slot,
                a,
                &extra_terms,
                type_produced,
                multiplier,
            )?);
        }
        if !options.is_empty() {
            permanents.push(options);
        }
    }
    Some(permanents)
}

type ReferenceChanges = (Vec<(u8, u8, bool)>, u8, u8);

/// Reads live mana-changing continuous effects from the Effects store and permanent
/// `AbilityKind::ManaChange` abilities from the battlefield (independent of `mana::changes` and
/// `mana::rich`), returning `(fixed_extra_terms, type_produced_count, multiplier)` when `slot`
/// is tapped for mana, or `None` if the product of multipliers overflows `u8`.
#[allow(
    clippy::disallowed_methods,
    reason = "reads the source card's printed face to look up the ChangeMana instruction data"
)]
fn live_mana_changes<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<ReferenceChanges> {
    let mut terms = Vec::new();
    let mut type_produced = 0u8;
    let mut multiplier = 1u8;
    let tapped_controller = s.controller(slot);
    let mut record = |added: Added| match added {
        Added::Of(mana) => terms.push((mana.colors, mana.amount, false)),
        // Bounded by the number of live mana-changing effects in the debug reference checker.
        Added::TypeProduced => type_produced = type_produced.saturating_add(1),
    };
    for entry in s.effects() {
        let Payload::Mana(effect) = entry.payload else {
            continue;
        };
        let face = t.def(entry.source).face(effect.face);
        let ability = &face.abilities[effect.ability.index()];
        let Behavior::Data(root) = ability.body() else {
            continue;
        };
        let Some(change) = root.mana_change_at(effect.part) else {
            continue;
        };
        let words = Words::of_changes(effect.colors, effect.lands);
        match *change {
            ManaChange::Adds {
                tapped: Tapped::A(filter),
                added,
            } if filter.matches(s, t, slot, effect.controller, &words) => record(added),
            ManaChange::Multiplies { tapped, by }
                if tapped_controller == effect.controller
                    && tapped.matches(s, t, slot, effect.controller, &words) =>
            {
                multiplier = multiplier.checked_mul(by)?;
            }
            ManaChange::Adds { .. } | ManaChange::Multiplies { .. } => {}
        }
    }
    for &source_slot in s.battlefield() {
        let source_controller = s.controller(source_slot);
        for (_, a) in s.abilities(t, source_slot) {
            let Some(change) = a.changes_mana() else {
                continue;
            };
            match *change {
                ManaChange::Adds { tapped, added } => {
                    let matches = match tapped {
                        Tapped::A(filter) => {
                            let words = s.words_of(source_slot);
                            filter.matches(s, t, slot, source_controller, &words)
                        }
                        Tapped::AttachedTo => s.object(source_slot).attached_to() == Some(slot),
                    };
                    if matches {
                        record(added);
                    }
                }
                ManaChange::Multiplies { tapped, by } => {
                    let words = s.words_of(source_slot);
                    if tapped_controller == source_controller
                        && tapped.matches(s, t, slot, source_controller, &words)
                    {
                        multiplier = multiplier.checked_mul(by)?;
                    }
                }
            }
        }
    }
    Some((terms, type_produced, multiplier))
}

/// Independent least-fixed-point computation of the mana types permanents matching `filter`
/// could produce (see CR 106.7), sharing no code with `mana::changes::could_produce`.
fn reference_could_produce<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    controller: Seat,
    filter: &beryl_card::Filter,
    words: &Words,
) -> u8 {
    let battlefield = s.battlefield();
    let mut kinds = vec![0u8; battlefield.len()];
    for (i, &slot) in battlefield.iter().enumerate() {
        let mut mask = s.intrinsic_mana(t, slot);
        for (_, a) in s.abilities(t, slot) {
            if a.kind() != AbilityKind::Mana {
                continue;
            }
            for term in a.product() {
                let n = if let Amount::Fixed(n) = term.amount {
                    n
                } else {
                    crate::interpret::amount_of(s, t, slot, term.amount).unwrap_or(1)
                };
                if n > 0
                    && let beryl_card::Kinds::Fixed(bits) = term.kinds
                {
                    mask |= bits;
                }
            }
        }
        kinds[i] = mask;
    }
    for _ in 0..6 {
        let mut changed = false;
        for (i, &slot) in battlefield.iter().enumerate() {
            let mut extra = 0u8;
            let c = s.controller(slot);
            let w = s.words_of(slot);
            for (_, a) in s.abilities(t, slot) {
                if a.kind() != AbilityKind::Mana {
                    continue;
                }
                for term in a.product() {
                    let beryl_card::Kinds::CouldProduce(sub) = term.kinds else {
                        continue;
                    };
                    let n = if let Amount::Fixed(n) = term.amount {
                        n
                    } else {
                        crate::interpret::amount_of(s, t, slot, term.amount).unwrap_or(1)
                    };
                    if n == 0 {
                        continue;
                    }
                    for (j, &other) in battlefield.iter().enumerate() {
                        if sub.matches(s, t, other, c, &w) {
                            extra |= kinds[j];
                        }
                    }
                }
            }
            if kinds[i] | extra != kinds[i] {
                kinds[i] |= extra;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    battlefield
        .iter()
        .enumerate()
        .filter(|(_, slot)| filter.matches(s, t, **slot, controller, words))
        .fold(0u8, |acc, (i, _)| acc | kinds[i])
}

/// The activation of a mana ability on `slot`, read from its cost and product: `None` for a cost
/// with a part besides {T} and sacrificing its source.
fn activation_on<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    a: &crate::card::Ability,
    extra_terms: &[(u8, u8, bool)],
    type_produced: u8,
    multiplier: u8,
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
    let mut own_terms = Vec::new();
    let words = s.words_of(slot);
    for term in a.product() {
        let n = if let Amount::Fixed(n) = term.amount {
            n
        } else {
            crate::interpret::amount_of(s, t, slot, term.amount).ok()?
        };
        if n == 0 {
            continue;
        }
        let kinds = match term.kinds {
            beryl_card::Kinds::Fixed(bits) => bits,
            beryl_card::Kinds::CouldProduce(filter) => {
                reference_could_produce(s, t, s.controller(slot), filter, &words)
            }
        };
        if kinds == 0 {
            continue;
        }
        if multiplier > 1 && !term.uniform && !kinds.is_power_of_two() && n > 1 {
            for _ in 0..n {
                own_terms.push((kinds, multiplier, true));
            }
        } else {
            let scaled = n.checked_mul(multiplier)?;
            let uniform = term.uniform || (multiplier > 1 && !kinds.is_power_of_two());
            own_terms.push((kinds, scaled, uniform));
        }
    }
    let mut price = a.cost().mana;
    if super::reference_spends_as_any_color(s, t, s.controller(slot), slot) {
        price = price.as_any_color();
    }
    if type_produced == 0 {
        own_terms.extend_from_slice(extra_terms);
        return Some(Activation::of(price, &own_terms));
    }
    // Expand each own-term choice down to concrete single-color units so we know the exact set
    // of mana types the tapped ability itself produced, then append fixed extras and
    // `type_produced` units of `produced_mask`.
    let mut makes: Vec<Vec<u8>> = vec![Vec::new()];
    for &(kinds, amount, uniform) in &own_terms {
        let single_kinds: Vec<u8> = (0..6)
            .map(|k| 1u8 << k)
            .filter(|k| kinds & k != 0)
            .collect();
        if uniform || amount == 1 || single_kinds.len() == 1 {
            makes = makes
                .iter()
                .flat_map(|made| {
                    single_kinds.iter().map(move |&kind| {
                        let mut more = made.clone();
                        more.extend(std::iter::repeat_n(kind, usize::from(amount)));
                        more
                    })
                })
                .collect();
        } else {
            for _ in 0..amount {
                makes = makes
                    .iter()
                    .flat_map(|made| {
                        single_kinds.iter().map(move |&kind| {
                            let mut more = made.clone();
                            more.push(kind);
                            more
                        })
                    })
                    .collect();
            }
        }
    }
    let mut result_makes = Vec::with_capacity(makes.len());
    for mut made in makes {
        let produced_mask = made.iter().copied().fold(0u8, |acc, k| acc | k);
        for &(kinds, amount, _) in extra_terms {
            made.extend(std::iter::repeat_n(kinds, usize::from(amount)));
        }
        if produced_mask != 0 {
            made.extend(std::iter::repeat_n(
                produced_mask,
                usize::from(type_produced),
            ));
        }
        if !result_makes.contains(&made) {
            result_makes.push(made);
        }
    }
    Some(Activation {
        price,
        makes: result_makes,
    })
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
        let (beryl_card::Kinds::Fixed(bits), Amount::Fixed(n)) = (term.kinds, term.amount) else {
            return None;
        };
        terms.push((bits, n, term.uniform));
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
