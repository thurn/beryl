//! The mana solver's path for what its classes of sources cannot say: mana abilities with a
//! price ([`crate::table::is_priced`]) and products of several terms or of a counted amount
//! ([`crate::card::Ability::product`]). It is entered only in a game whose cards have one
//! ([`Capability::PricedMana`], [`Capability::ManaChanges`]), and only where the plain path
//! found no payment ([`super::Funds::pays_for`]) or after it listed its plans
//! ([`super::plans_accepting`]), so a game without these cards runs none of it.
//!
//! A payment here is a sequence: mana abilities activated in order, each paying its own price
//! from the mana in the pool as it then stands, the floating mana and what earlier activations
//! made, and then the cost paid from the pool (see [CR 605.3a]): "A player may activate an
//! activated mana ability whenever they have priority, whenever they are casting a spell or
//! activating an ability that requires a mana payment, or whenever a rule or effect asks for a
//! mana payment".
//!
//! Implements: none. Plans and carries out sequences of priced or multi-term mana abilities.
//!
//! [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a

mod manual;
mod plans;
mod terms;
mod walk;

use std::collections::BTreeSet;

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, AbilityKind, Amount, CostAction, CostPart, INTRINSIC_MANA, ManaColor,
    ManaCost, ManaSet, ObjectAbility, Timing,
};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::replacement::event::{self, Sink};
use crate::state::{AbilityRef, Derived, ManaAmounts, PendingTrigger, State, TriggeredAbility};
use crate::table::{Capability, GameTable, is_priced, is_sacrificed};

use super::pay::{activate_consuming, activate_restricted, human_taps, tapped_source};
use super::pool::Funds;
use super::remembered::{self, Paying};
use super::solver::Plan;
use super::sources::{Sources, allowed, can_tap};
use super::units::Units;
use super::{CLASSES, KINDS, Taps, amounts_pay};

pub(super) use manual::{manual_can_spend_after, manual_extra_options, manual_pay_extra};
pub(super) use plans::{each_plan, plans};
pub(super) use terms::{EvaluatedTerm, RichActivation};

/// A class of interchangeable sources the plain classes cannot say: permanents with a priced mana
/// ability ([`is_priced`]) or a mana ability whose product one [`ManaSet`] cannot say.
#[derive(Clone, Debug)]
pub(crate) struct Extra<L: Layout = Small> {
    pub(super) members: Bitboard<L::Slot>,
    pub(super) count: u8,
    pub(super) least: crate::query::DefinitionKey<L::Card>,
    pub(super) activations: Vec<RichActivation>,
}

/// One activation in a rich plan's sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Step {
    pub(super) extra: u8,
    pub(super) activation: u8,
    pub(super) price_spend: [u8; KINDS],
    pub(super) made: [u8; KINDS],
}

/// The sequence of activations a rich [`Plan`] performs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Sequence {
    /// `(class_index, color_index)` for each plain source tapped before the sequence.
    pub(super) plain: Vec<(u8, u8)>,
    pub(super) steps: Vec<Step>,
    /// Concrete mana spent from the pool on the final cost.
    pub(super) final_spend: [u8; KINDS],
}

/// Whether the permanent in `slot` has any priced mana ability, multi-term/counted mana
/// ability, or live mana-changing effect that applies to it.
#[cold]
#[inline(never)]
pub(super) fn has_extra_mana<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    if s.has_mana_changes() && super::changes::matches_any(s, t, slot) {
        return true;
    }
    s.abilities(t, slot).any(|(_, a)| {
        is_priced(a)
            || (a.kind() == AbilityKind::Mana
                && a.produces() == ManaSet::NONE
                && !crate::table::is_could_produce(a))
    })
}

/// Collects all untapped permanents `player` controls (not in `exclude`) that have a priced or
/// multi-term/counted mana ability into `extras`. Out of line: called only in a game with
/// `PricedMana` or `ManaChanges`.
#[cold]
#[inline(never)]
pub(super) fn collect_extras<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
    extras: &mut Vec<Extra<L>>,
) -> Result<(), Full> {
    for (slot, d) in super::sources::controlled(s, player) {
        if exclude.contains(slot) || super::sources::source(s, t, player, slot, d).is_some() {
            continue;
        }
        collect_extra(s, t, player, slot, d, extras)?;
    }
    Ok(())
}

fn collect_extra<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
    extras: &mut Vec<Extra<L>>,
) -> Result<(), Full> {
    if !can_tap(s, player, slot, d) {
        return Ok(());
    }
    let words = s.words_of(slot);
    let changes = if s.has_mana_changes() {
        super::changes::matching(s, t, slot)?
    } else {
        super::changes::MatchingChanges::default()
    };
    let mut activations = Vec::new();
    let intrinsic = s.intrinsic_mana(t, slot);
    for k in 0..KINDS {
        if intrinsic & (1 << k) != 0 {
            let mut terms = [EvaluatedTerm::default(); 8];
            terms[0] = EvaluatedTerm {
                kinds: 1 << k,
                amount: changes.multiplier,
                uniform: false,
            };
            let mut count = 1usize;
            super::changes::append_extra_mana(&mut terms, 1, &mut count, changes.fixed)?;
            activations.push(RichActivation {
                ability: None,
                price: ManaCost::NONE,
                terms,
                own_term_count: 1,
                term_count: u8::try_from(count).expect("fits u8"),
                type_produced: changes.type_produced,
                consumed: false,
            });
        }
    }
    for (obj_ability, a) in s.abilities(t, slot) {
        if a.kind() != AbilityKind::Mana
            || a.timing() == Timing::AsInstant
            || a.spend().is_some()
            || !allowed(s, t, a, player, &words)
        {
            continue;
        }
        let Some(act) = eval_activation(s, t, slot, obj_ability, a, changes)? else {
            continue;
        };
        if act.total_mana() > 0 {
            activations.push(act);
        }
    }
    if activations.is_empty() {
        return Ok(());
    }
    let key = s.definition(slot).key();
    let card_mana = s.makes_card_mana(t, slot);
    if card_mana
        && let Some(existing) = extras
            .iter_mut()
            .find(|e| e.least == key && e.count < u8::MAX && e.activations == activations)
    {
        existing.members.insert(slot);
        existing.count += 1;
        return Ok(());
    }
    if extras.len() >= CLASSES {
        return Err(Full);
    }
    extras.push(Extra {
        members: Bitboard::single(slot),
        count: 1,
        least: key,
        activations,
    });
    Ok(())
}

fn eval_activation<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    obj_ability: ObjectAbility,
    a: &Ability,
    changes: super::changes::MatchingChanges,
) -> Result<Option<RichActivation>, Full> {
    let sacrificed = is_sacrificed(a);
    let tap_and_mana =
        a.cost().mana.x == 0 && matches!(a.cost().parts(), [CostPart::This(CostAction::Tap)]);
    if !sacrificed && !tap_and_mana {
        return Ok(None);
    }
    let mut terms = [EvaluatedTerm::default(); 8];
    let mut count = 0usize;
    let tapped = a.cost().does(CostAction::Tap);
    let multiplier = if tapped { changes.multiplier } else { 1 };
    let words = s.words_of(slot);
    for term in a.product() {
        let amount = if let Amount::Fixed(n) = term.amount {
            n
        } else {
            crate::interpret::amount_of(s, t, slot, term.amount)
        };
        if amount == 0 {
            continue;
        }
        let kinds = match term.kinds {
            beryl_card::Kinds::Fixed(bits) => bits,
            beryl_card::Kinds::CouldProduce(filter) => {
                super::changes::could_produce(s, t, s.controller(slot), filter, &words)
            }
        };
        if kinds == 0 {
            continue;
        }
        if multiplier > 1 && !term.uniform && !kinds.is_power_of_two() && amount > 1 {
            for _ in 0..amount {
                if count >= terms.len() {
                    return Err(Full);
                }
                terms[count] = EvaluatedTerm {
                    kinds,
                    amount: multiplier,
                    uniform: true,
                };
                count += 1;
            }
        } else {
            let scaled = amount.checked_mul(multiplier).ok_or(Full)?;
            let uniform = term.uniform || (multiplier > 1 && !kinds.is_power_of_two());
            if count >= terms.len() {
                return Err(Full);
            }
            terms[count] = EvaluatedTerm {
                kinds,
                amount: scaled,
                uniform,
            };
            count += 1;
        }
    }
    let own_term_count = count;
    let type_produced = if tapped {
        super::changes::append_extra_mana(&mut terms, own_term_count, &mut count, changes.fixed)?;
        changes.type_produced
    } else {
        0
    };
    let mut price = a.cost().mana;
    if crate::continuous::rule_change::spends_as_any_color(s, t, s.controller(slot), slot) {
        price = price.as_any_color();
    }
    Ok(Some(RichActivation {
        ability: Some(obj_ability),
        price,
        terms,
        own_term_count: u8::try_from(own_term_count).expect("fits u8"),
        term_count: u8::try_from(count).expect("fits u8"),
        type_produced,
        consumed: sacrificed,
    }))
}

/// Whether `player`, whose funds the plain path found are `funds`, can pay `cost` with none of
/// `exclude` for what `paying` is for by activating priced mana abilities or abilities whose
/// product the classes cannot say, where the plain path found they cannot
/// ([`Funds::pays_for`]).
#[cold]
#[inline(never)]
pub(super) fn pays<L: Layout>(
    funds: &Funds<L>,
    s: &State<L>,
    t: &GameTable,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> bool {
    let Ok(sources) = Sources::of(s, t, funds.player(), exclude, paying) else {
        // More classes than Small holds: say it may be paid so the payment's own scan widens.
        return true;
    };
    pays_with_sources(s, funds.player(), cost, &sources)
}

/// [`pays`] with `sources` already built.
#[cold]
#[inline(never)]
pub(super) fn pays_with_sources<L: Layout>(
    s: &State<L>,
    player: Seat,
    cost: ManaCost,
    sources: &Sources<L>,
) -> bool {
    if sources.extras.is_empty() {
        return false;
    }
    let pool = s.player(player).mana().amounts_u8();
    pays_from_state(pool, sources, &Table::filled(0), 0, &[0; CLASSES], cost)
}

/// Whether `cost` is payable starting from `pool`, the untapped plain sources not in `taps`, the
/// remembered mana not in `rem_used`, and the extras not in `extra_used`.
pub(super) fn pays_from_state<L: Layout>(
    pool: [u8; KINDS],
    sources: &Sources<L>,
    taps: &Taps<L>,
    rem_used: u8,
    extra_used: &[u8; CLASSES],
    cost: ManaCost,
) -> bool {
    pays_from_state_with(pool, sources, taps, rem_used, extra_used, None, cost)
}

/// [`pays_from_state`] with an optional unplayed land `(kinds, amount)` the look-ahead considers.
pub(super) fn pays_from_state_with<L: Layout>(
    pool: [u8; KINDS],
    sources: &Sources<L>,
    taps: &Taps<L>,
    rem_used: u8,
    extra_used: &[u8; CLASSES],
    bonus_land: Option<(u8, u8)>,
    cost: ManaCost,
) -> bool {
    let mut base = Units::NONE;
    for (k, &n) in pool.iter().enumerate() {
        base.add(1 << k, u16::from(n));
    }
    if let Some((kinds, amount)) = bonus_land {
        base.add(kinds, u16::from(amount));
    }
    let mut restricted = Units::NONE;
    for (i, &(_, kind, _)) in sources.remembered().iter().enumerate() {
        if rem_used & (1 << i) == 0 {
            restricted.add(1 << kind, 1);
        }
    }
    let mut splits = Vec::new();
    let classes = sources.classes();
    for (i, c) in classes.iter().enumerate() {
        if c.is_twin() {
            continue;
        }
        let avail = c
            .count()
            .saturating_sub(taps[i])
            .saturating_sub(c.twin().map_or(0, |tw| taps[usize::from(tw)]));
        if avail == 0 {
            continue;
        }
        if c.is_restricted() {
            restricted.add(c.kinds, u16::from(c.amount) * u16::from(avail));
        } else if let Some(tw) = c.twin() {
            let other = &classes[usize::from(tw)];
            if other.is_restricted() {
                // A permanent with both unrestricted and restricted mana abilities, as Cavern
                // of Souls: tapped for the first, its mana may pay a price; tapped for the
                // second, only the cost, which either's mana pays (see CR 106.6).
                let (kinds, amount) =
                    super::sources::capacity(c.kinds, c.amount, Some((other.kinds, other.amount)));
                splits.push(Split {
                    plain: (c.kinds, c.amount),
                    either: (kinds, amount),
                    avail,
                });
            } else {
                let (kinds, amount) =
                    super::sources::capacity(c.kinds, c.amount, Some((other.kinds, other.amount)));
                base.add(kinds, u16::from(amount) * u16::from(avail));
            }
        } else {
            base.add(c.kinds, u16::from(c.amount) * u16::from(avail));
        }
    }
    // A permanent whose mana abilities all lack a price is tapped up front: more mana never
    // makes a cost unpayable, and what it makes may pay a price. Only the order of priced
    // activations matters, so the search below tries orders of those alone. Each starting
    // pool is one way of choosing what those permanents make, kept once.
    let mut used = *extra_used;
    let mut starts = BTreeSet::from([base]);
    for (i, extra) in sources.extras.iter().enumerate() {
        let left = extra.count.saturating_sub(used[i]);
        if left == 0 || extra.activations.iter().any(|a| !a.price.is_zero()) {
            continue;
        }
        let mut outcomes = BTreeSet::new();
        for act in &extra.activations {
            act.each_units(Units::NONE, &mut |made| {
                outcomes.insert(made);
                true
            });
        }
        for _ in 0..left {
            starts = starts
                .iter()
                .flat_map(|&start| outcomes.iter().map(move |&made| add_units(start, made)))
                .collect();
        }
        used[i] = extra.count;
    }
    starts
        .into_iter()
        .any(|start| split_pays(start, restricted, &splits, used, &sources.extras, cost))
}

/// The members of a class with a restricted twin class ([`super::sources::Class::twin`]): how
/// many are untapped, and the kinds and amount of mana one makes tapped for its unrestricted
/// mana, which may pay a price, and for either, which pays only the cost.
struct Split {
    plain: (u8, u8),
    either: (u8, u8),
    avail: u8,
}

/// [`pays_dfs`] for each way of dividing the members of each of `splits` between mana that may
/// pay a price, added to `base`, and mana that pays the cost, added to `restricted`.
fn split_pays<L: Layout>(
    base: Units,
    restricted: Units,
    splits: &[Split],
    used: [u8; CLASSES],
    extras: &[Extra<L>],
    cost: ManaCost,
) -> bool {
    let Some((split, rest)) = splits.split_first() else {
        let mut seen: BTreeSet<([u8; CLASSES], Units)> = BTreeSet::new();
        return pays_dfs(base, restricted, used, extras, cost, &mut seen);
    };
    (0..=split.avail).any(|n| {
        let mut base = base;
        let mut restricted = restricted;
        base.add(split.plain.0, u16::from(split.plain.1) * u16::from(n));
        restricted.add(
            split.either.0,
            u16::from(split.either.1) * u16::from(split.avail - n),
        );
        split_pays(base, restricted, rest, used, extras, cost)
    })
}

fn add_units(mut a: Units, b: Units) -> Units {
    for (kinds, n) in b.nonempty() {
        a.add(kinds, n);
    }
    a
}

fn pays_dfs<L: Layout>(
    pool: Units,
    restricted: Units,
    used: [u8; CLASSES],
    extras: &[Extra<L>],
    cost: ManaCost,
    seen: &mut BTreeSet<([u8; CLASSES], Units)>,
) -> bool {
    if add_units(pool, restricted).pays(cost) {
        return true;
    }
    if !seen.insert((used, pool)) {
        return false;
    }
    for (i, extra) in extras.iter().enumerate() {
        if used[i] >= extra.count {
            continue;
        }
        let mut next_used = used;
        next_used[i] += 1;
        for act in &extra.activations {
            if !act.price.is_zero() && !pool.pays(act.price) {
                continue;
            }
            let mut found = false;
            each_units_payment(pool, act.price, &mut |left| {
                act.each_units(left, &mut |next_pool| {
                    if pays_dfs(next_pool, restricted, next_used, extras, cost, seen) {
                        found = true;
                        return false;
                    }
                    true
                })
            });
            if found {
                return true;
            }
        }
    }
    false
}

fn each_units_payment(pool: Units, price: ManaCost, f: &mut impl FnMut(Units) -> bool) -> bool {
    let total = price.total(0);
    if total == 0 {
        return f(pool);
    }
    let buckets: Vec<(u8, u16)> = pool.nonempty().collect();
    let mut counts = vec![0u16; buckets.len()];
    choose_units_payment(&buckets, 0, total, price, &mut counts, pool, f)
}

fn choose_units_payment(
    buckets: &[(u8, u16)],
    at: usize,
    left: u32,
    price: ManaCost,
    counts: &mut [u16],
    pool: Units,
    f: &mut impl FnMut(Units) -> bool,
) -> bool {
    if left == 0 {
        let mut spent = Units::NONE;
        for (&(kinds, _), &n) in buckets.iter().zip(counts.iter()) {
            if n > 0 {
                spent.add(kinds, n);
            }
        }
        if !spent.pays(price) {
            return true;
        }
        let mut rem = pool;
        for (&(kinds, _), &n) in buckets.iter().zip(counts.iter()) {
            if n > 0 {
                rem.take(kinds, n);
            }
        }
        return f(rem);
    }
    if at == buckets.len() {
        return true;
    }
    let avail = u32::from(buckets[at].1);
    let max_take = avail.min(left);
    for n in 0..=max_take {
        counts[at] = u16::try_from(n).expect("fits u16");
        if !choose_units_payment(buckets, at + 1, left - n, price, counts, pool, f) {
            counts[at] = 0;
            return false;
        }
    }
    counts[at] = 0;
    true
}

/// Carries out a rich plan's sequence inside the payment's event group (`mana::pay`).
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    _cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    plan: &Plan<L>,
    paying: Paying<L>,
) -> Result<ManaAmounts, Full> {
    let seq = plan
        .sequence
        .as_deref()
        .expect("a rich plan has a sequence");
    let sources = Sources::of(s, t, player, exclude, paying)?;
    let mut spent = seq.final_spend;
    if plan.remembered != 0 {
        for (i, &(at, kind, _)) in sources.remembered().iter().enumerate().rev() {
            if plan.remembered & (1 << i) != 0 {
                remembered::spend(s, t, player, usize::from(at), paying)?;
                spent[usize::from(kind)] = spent[usize::from(kind)].saturating_add(1);
            }
        }
    }
    let human = s.profile().taps_mana == Some(player);
    let mut class_used = [0usize; CLASSES];
    let mut class_slots: Vec<Vec<L::Slot>> = Vec::with_capacity(sources.classes().len());
    for (i, c) in sources.classes().iter().enumerate() {
        let slots = if human {
            human_taps(s, c.members, c.count(), plan.taps[i])?
        } else {
            s.mana_payment_sources(c.members, plan.taps[i])?
        };
        class_slots.push(slots.as_slice().to_vec());
    }
    for &(ci, color_ix) in &seq.plain {
        let ci = usize::from(ci);
        let c = &sources.classes()[ci];
        let slot = class_slots[ci][class_used[ci]];
        class_used[ci] += 1;
        let color = ManaColor::ALL[usize::from(color_ix)];
        if c.is_restricted() {
            activate_restricted(s, t, sink, tapped_source(s, slot), color)?;
            let last = s.player(player).remembered_mana().len() - 1;
            remembered::spend(s, t, player, last, paying)?;
            spent[usize::from(color_ix)] = spent[usize::from(color_ix)].saturating_add(c.amount);
        } else {
            activate_consuming(s, t, sink, tapped_source(s, slot), color, c.is_consumed())?;
        }
    }
    let mut extra_counts = [0u8; CLASSES];
    for step in &seq.steps {
        extra_counts[usize::from(step.extra)] += 1;
    }
    let mut extra_slots: Vec<Vec<L::Slot>> = Vec::with_capacity(sources.extras.len());
    for (i, e) in sources.extras.iter().enumerate() {
        let slots = if human {
            human_taps(s, e.members, e.count, extra_counts[i])?
        } else {
            s.mana_payment_sources(e.members, extra_counts[i])?
        };
        extra_slots.push(slots.as_slice().to_vec());
    }
    let mut extra_used = [0usize; CLASSES];
    for step in &seq.steps {
        let ei = usize::from(step.extra);
        let slot = extra_slots[ei][extra_used[ei]];
        extra_used[ei] += 1;
        let act = &sources.extras[ei].activations[usize::from(step.activation)];
        activate_extra_step(s, t, sink, player, slot, act, step.price_spend, step.made)?;
    }
    for (k, &n) in seq.final_spend.iter().enumerate() {
        if n > 0 {
            s.spend_mana(player, ManaColor::ALL[k], n);
        }
    }
    debug_assert_eq!(s.player(player).mana().amounts_u8(), plan.left);
    Ok(spent)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn activate_extra_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    act: &RichActivation,
    price_spend: [u8; KINDS],
    made: [u8; KINDS],
) -> Result<(), Full> {
    for (k, &n) in price_spend.iter().enumerate() {
        if n > 0 {
            s.spend_mana(player, ManaColor::ALL[k], n);
        }
    }
    let permanent = tapped_source(s, slot);
    let has_changes =
        act.ability.is_none() && t.can(Capability::ManaChanges) && s.has_mana_changes();
    let (multiplier, extra) = if has_changes {
        let color_ix = act.terms()[0].kinds.trailing_zeros() as usize;
        let changes = super::changes::matching(s, t, slot)?;
        (
            changes.multiplier,
            changes.add_for_single_color(ManaColor::ALL[color_ix])?,
        )
    } else {
        (1, [0; KINDS])
    };
    event::set_tapped(s, t, sink, permanent, true)?;
    if act.consumed {
        event::move_one(
            s,
            t,
            sink,
            slot,
            crate::state::Zone::Graveyard,
            event::Reason::Sacrifice,
        )?;
    }
    let source: crate::state::ObjectRef<L> = permanent.into();
    let (card, face) = s.definition(slot).record(t);
    match act.ability {
        None => {
            let color_ix = act.terms()[0].kinds.trailing_zeros() as usize;
            let color = ManaColor::ALL[color_ix];
            let intrinsic = &INTRINSIC_MANA[color_ix];
            let activation = || PendingTrigger {
                of: AbilityRef {
                    source: Some(source),
                    controller: player.into(),
                    ability: TriggeredAbility::own(AbilityIx::new(u8::MAX)),
                    card,
                    face,
                },
                ..PendingTrigger::default()
            };
            if multiplier > 1 {
                crate::interpret::mana_ability_multiplied(
                    s, t, sink, activation, intrinsic, color, multiplier,
                )?;
            } else {
                crate::interpret::mana_ability(s, t, sink, activation, intrinsic, color)?;
            }
            if has_changes {
                super::changes::add_extra(s, sink, permanent, player, extra);
            }
            Ok(())
        }
        Some(obj_ability) => {
            let ability = s
                .abilities(t, slot)
                .find(|&(id, _)| id == obj_ability)
                .map(|(_, a)| a)
                .expect("activated mana ability");
            crate::coverage::hit(ability);
            for (k, &n) in made.iter().enumerate() {
                if n > 0 {
                    let added = s.add_mana(player, Some(source), ManaColor::ALL[k], n);
                    sink.note_from(Some(source), added);
                }
            }
            crate::continuous::layers::refresh(s, t, sink)
        }
    }
}

/// Enumerates concrete `[u8; KINDS]` ways to pay `price` from `pool` alone.
pub(super) fn each_pool_payment(
    pool: [u8; KINDS],
    price: ManaCost,
    f: &mut impl FnMut([u8; KINDS], [u8; KINDS]) -> bool,
) {
    let total = price.total(0);
    if total == 0 {
        f([0; KINDS], pool);
        return;
    }
    let mut spend = [0u8; KINDS];
    choose_pool_payment(pool, 0, total, price, &mut spend, f);
}

fn choose_pool_payment(
    pool: [u8; KINDS],
    k: usize,
    left: u32,
    price: ManaCost,
    spend: &mut [u8; KINDS],
    f: &mut impl FnMut([u8; KINDS], [u8; KINDS]) -> bool,
) -> bool {
    if left == 0 {
        if !amounts_pay(*spend, price) {
            return true;
        }
        let mut after = pool;
        for (a, &s) in after.iter_mut().zip(spend.iter()) {
            *a -= s;
        }
        return f(*spend, after);
    }
    if k == KINDS {
        return true;
    }
    let max_n = u32::from(pool[k]).min(left);
    for n in (0..=max_n).rev() {
        spend[k] = u8::try_from(n).expect("fits u8");
        if !choose_pool_payment(pool, k + 1, left - n, price, spend, f) {
            spend[k] = 0;
            return false;
        }
    }
    spend[k] = 0;
    true
}
