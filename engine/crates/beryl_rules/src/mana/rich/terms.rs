//! Evaluated terms and concrete outcome enumeration for `mana::rich`.
//!
//! Implements: none. Enumerates mana assignments for rich mana abilities.

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityKind, Amount, CostAction, CostPart, ManaCost, ObjectAbility, Timing,
};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Derived, State};
use crate::table::{GameTable, is_sacrificed};

use super::super::remembered::{self, Paying};
use super::super::sources::{allowed, can_tap};
use super::super::units::Units;
use super::super::{CLASSES, KINDS};
use super::Extra;

/// One evaluated term of a mana ability's product: `amount` units of mana of `kinds`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(in crate::mana) struct EvaluatedTerm {
    pub(in crate::mana) kinds: u8,
    pub(in crate::mana) amount: u8,
    pub(in crate::mana) uniform: bool,
}

/// One mana ability on an [`super::Extra`] permanent, with its terms evaluated for the board now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::mana) struct RichActivation {
    /// `None` for the intrinsic mana ability of a basic land type (see CR 305.6).
    pub(super) ability: Option<ObjectAbility>,
    pub(super) price: ManaCost,
    pub(super) terms: [EvaluatedTerm; 8],
    /// Leading terms produced by the tapped ability itself (`terms[..own_term_count]`);
    /// fixed additions from [`beryl_card::Added::Of`] follow in `terms[own_term_count..term_count]`.
    pub(super) own_term_count: u8,
    pub(super) term_count: u8,
    /// Count of [`beryl_card::Added::TypeProduced`] triggers matching this tap.
    pub(super) type_produced: u8,
    pub(super) consumed: bool,
    /// Whether the ability's own mana is restricted (see CR 106.6): its `own_terms` pay only
    /// the cost, while `extra_terms` and `type_produced` from mana-changing triggers are
    /// unrestricted.
    pub(super) restricted: bool,
    /// Whether the ability's restriction attaches a rider to the spell it pays for.
    pub(super) rider: bool,
}

impl RichActivation {
    pub(super) fn terms(&self) -> &[EvaluatedTerm] {
        &self.terms[..usize::from(self.term_count)]
    }

    fn own_terms(&self) -> &[EvaluatedTerm] {
        &self.terms[..usize::from(self.own_term_count)]
    }

    fn extra_terms(&self) -> &[EvaluatedTerm] {
        &self.terms[usize::from(self.own_term_count)..usize::from(self.term_count)]
    }

    pub(in crate::mana) fn total_mana(&self) -> u32 {
        let own: u32 = self.own_terms().iter().map(|t| u32::from(t.amount)).sum();
        let extra: u32 = self.extra_terms().iter().map(|t| u32::from(t.amount)).sum();
        own + extra
            + if own > 0 {
                u32::from(self.type_produced)
            } else {
                0
            }
    }

    /// Calls `f` with each concrete `[u8; KINDS]` color assignment this activation can produce.
    pub(super) fn each_made(
        &self,
        f: &mut impl FnMut([u8; KINDS]) -> bool,
    ) -> Result<bool, crate::fixed::Full> {
        let mut made = [0u8; KINDS];
        if self.type_produced == 0 {
            return assign_terms(self.terms(), 0, &mut made, f);
        }
        assign_terms(self.own_terms(), 0, &mut made, &mut |own_made| {
            let mut produced_mask = 0u8;
            for (k, &n) in own_made.iter().enumerate() {
                if n > 0 {
                    produced_mask |= 1 << k;
                }
            }
            let mut total = own_made;
            for term in self.extra_terms() {
                let k = term.kinds.trailing_zeros() as usize;
                let Some(sum) = total[k].checked_add(term.amount) else {
                    return false;
                };
                total[k] = sum;
            }
            if produced_mask == 0 {
                return f(total);
            }
            assign_each_of(produced_mask, 0, self.type_produced, &mut total, &mut |m| {
                Ok(f(*m))
            })
            .unwrap_or(false)
        })
    }

    /// Calls `f` with each `(unrestricted, restricted)` concrete color assignment this activation
    /// can produce. When `self.restricted` is false, `restricted` is all zeros; when true,
    /// `own_terms` are restricted and `extra_terms` + `type_produced` are unrestricted.
    pub(super) fn each_made_split(
        &self,
        f: &mut impl FnMut([u8; KINDS], [u8; KINDS]) -> bool,
    ) -> Result<bool, Full> {
        if !self.restricted {
            return self.each_made(&mut |made| f(made, [0; KINDS]));
        }
        let mut own = [0u8; KINDS];
        assign_terms(self.own_terms(), 0, &mut own, &mut |own_made| {
            let mut produced_mask = 0u8;
            for (k, &n) in own_made.iter().enumerate() {
                if n > 0 {
                    produced_mask |= 1 << k;
                }
            }
            let mut extra = [0u8; KINDS];
            for term in self.extra_terms() {
                let k = term.kinds.trailing_zeros() as usize;
                let Some(sum) = extra[k].checked_add(term.amount) else {
                    return false;
                };
                extra[k] = sum;
            }
            if self.type_produced == 0 || produced_mask == 0 {
                return f(extra, own_made);
            }
            assign_each_of(produced_mask, 0, self.type_produced, &mut extra, &mut |m| {
                Ok(f(*m, own_made))
            })
            .unwrap_or(false)
        })
    }

    /// Calls `f` with each `Units` representation this activation can produce (branching only on
    /// `uniform` multi-kind terms, or concrete outcomes when `type_produced > 0`).
    pub(super) fn each_units(&self, base: Units, f: &mut impl FnMut(Units) -> bool) -> bool {
        if self.type_produced == 0 {
            return assign_term_units(self.terms(), 0, base, f);
        }
        self.each_made(&mut |made| {
            let mut units = base;
            for (k, &n) in made.iter().enumerate() {
                if n > 0 {
                    units.add(1 << k, u16::from(n));
                }
            }
            f(units)
        })
        .unwrap_or(false)
    }

    /// Calls `f` with each `(unrestricted, restricted)` `Units` pair this activation can produce.
    pub(super) fn each_units_split(
        &self,
        base: Units,
        restricted: Units,
        f: &mut impl FnMut(Units, Units) -> bool,
    ) -> bool {
        if !self.restricted {
            return self.each_units(base, &mut |u| f(u, restricted));
        }
        self.each_made_split(&mut |unres, res| {
            let mut u = base;
            let mut r = restricted;
            for (k, (&nu, &nr)) in unres.iter().zip(&res).enumerate() {
                if nu > 0 {
                    u.add(1 << k, u16::from(nu));
                }
                if nr > 0 {
                    r.add(1 << k, u16::from(nr));
                }
            }
            f(u, r)
        })
        .unwrap_or(false)
    }
}

/// Collects all untapped permanents `player` controls (not in `exclude`) that have a priced or
/// multi-term/counted mana ability or live mana change into `extras`, including restricted mana
/// abilities accepted by `paying`. Returns whether any collected activation is restricted.
/// Out of line: called only in a game with `PricedMana` or `ManaChanges`.
#[cold]
#[inline(never)]
pub(in crate::mana) fn collect_extras<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
    extras: &mut Vec<Extra<L>>,
) -> Result<bool, Full> {
    let mut has_restricted = false;
    for (slot, d) in super::super::sources::controlled(s, player) {
        if exclude.contains(slot) || super::super::sources::source(s, t, player, slot, d).is_some()
        {
            continue;
        }
        has_restricted |= collect_extra(s, t, player, slot, d, paying, extras)?;
    }
    Ok(has_restricted)
}

fn collect_extra<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &Derived<L>,
    paying: Paying<L>,
    extras: &mut Vec<Extra<L>>,
) -> Result<bool, Full> {
    if !can_tap(s, player, slot, d) {
        return Ok(false);
    }
    let words = s.words_of(slot);
    let changes = if s.has_mana_changes() {
        super::super::changes::matching(s, t, slot)?
    } else {
        super::super::changes::MatchingChanges::default()
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
            super::super::changes::append_extra_mana(&mut terms, 1, &mut count, changes.fixed)?;
            activations.push(RichActivation {
                ability: None,
                price: ManaCost::NONE,
                terms,
                own_term_count: 1,
                term_count: u8::try_from(count).expect("fits u8"),
                type_produced: changes.type_produced,
                consumed: false,
                restricted: false,
                rider: false,
            });
        }
    }
    let reads_restrictions = paying.reads_restrictions(t);
    let mut has_restricted = false;
    for (obj_ability, a) in s.abilities(t, slot) {
        if a.kind() != AbilityKind::Mana
            || a.timing() == Timing::AsInstant
            || !allowed(s, t, a, player, &words)
        {
            continue;
        }
        let (restricted, rider) = if let Some(restriction) = a.spend() {
            if !reads_restrictions {
                continue;
            }
            let Some(id) = t.restriction_id(restriction) else {
                continue;
            };
            if !paying.accepts(t, id, remembered::chosen_type(s, slot)) {
                continue;
            }
            (true, restriction.rider.is_some())
        } else {
            (false, false)
        };
        let Some(act) = eval_activation(s, t, slot, obj_ability, a, changes, restricted, rider)?
        else {
            continue;
        };
        if act.total_mana() > 0 {
            has_restricted |= restricted;
            activations.push(act);
        }
    }
    if activations.is_empty() {
        return Ok(false);
    }
    let key = s.definition(slot).key();
    // Permanents of one card with the same activations are one extra, though an effect such as
    // Mana Flare or Urborg, Tomb of Yawgmoth changes what they make: the activations above
    // apply it, and each names the ability it activates, so one that lost or gained a mana
    // ability has different activations. A player may control more such lands than there is
    // room for extras.
    if let Some(existing) = extras
        .iter_mut()
        .find(|e| e.least == key && e.count < u8::MAX && e.activations == activations)
    {
        existing.members.insert(slot);
        existing.count += 1;
        return Ok(has_restricted);
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
    Ok(has_restricted)
}

#[allow(clippy::too_many_arguments)]
fn eval_activation<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    obj_ability: ObjectAbility,
    a: &Ability,
    changes: super::super::changes::MatchingChanges,
    restricted: bool,
    rider: bool,
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
            crate::interpret::amount_of(s, t, slot, term.amount)?
        };
        if amount == 0 {
            continue;
        }
        let kinds = match term.kinds {
            beryl_card::Kinds::Fixed(bits) => bits,
            beryl_card::Kinds::CouldProduce(filter) => {
                super::super::changes::could_produce(s, t, s.controller(slot), filter, &words)
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
        super::super::changes::append_extra_mana(
            &mut terms,
            own_term_count,
            &mut count,
            changes.fixed,
        )?;
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
        restricted,
        rider,
    }))
}

fn assign_terms(
    terms: &[EvaluatedTerm],
    ix: usize,
    made: &mut [u8; KINDS],
    f: &mut impl FnMut([u8; KINDS]) -> bool,
) -> Result<bool, crate::fixed::Full> {
    let Some(&term) = terms.get(ix) else {
        return Ok(f(*made));
    };
    if term.kinds.is_power_of_two() {
        let k = term.kinds.trailing_zeros() as usize;
        made[k] = made[k].checked_add(term.amount).ok_or(crate::fixed::Full)?;
        let cont = assign_terms(terms, ix + 1, made, f);
        made[k] -= term.amount;
        return cont;
    }
    if term.uniform {
        for k in 0..KINDS {
            if term.kinds & (1 << k) != 0 {
                made[k] = made[k].checked_add(term.amount).ok_or(crate::fixed::Full)?;
                let cont = assign_terms(terms, ix + 1, made, f);
                made[k] -= term.amount;
                if !cont? {
                    return Ok(false);
                }
            }
        }
        return Ok(true);
    }
    assign_each_of(term.kinds, 0, term.amount, made, &mut |m| {
        assign_terms(terms, ix + 1, m, f)
    })
}

fn assign_each_of(
    kinds: u8,
    from: usize,
    left: u8,
    made: &mut [u8; KINDS],
    f: &mut impl FnMut(&mut [u8; KINDS]) -> Result<bool, crate::fixed::Full>,
) -> Result<bool, crate::fixed::Full> {
    if left == 0 {
        return f(made);
    }
    for k in from..KINDS {
        if kinds & (1 << k) == 0 {
            continue;
        }
        let rest_mask = kinds & !((1u8 << (k + 1)).wrapping_sub(1));
        let min_n = if rest_mask == 0 { left } else { 0 };
        for n in (min_n..=left).rev() {
            made[k] = made[k].checked_add(n).ok_or(crate::fixed::Full)?;
            let cont = assign_each_of(kinds, k + 1, left - n, made, f);
            made[k] -= n;
            if !cont? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(true)
}

fn assign_term_units(
    terms: &[EvaluatedTerm],
    ix: usize,
    mut units: Units,
    f: &mut impl FnMut(Units) -> bool,
) -> bool {
    let Some(&term) = terms.get(ix) else {
        return f(units);
    };
    if !term.uniform || term.kinds.is_power_of_two() {
        units.add(term.kinds, u16::from(term.amount));
        return assign_term_units(terms, ix + 1, units, f);
    }
    for k in 0..KINDS {
        if term.kinds & (1 << k) != 0 {
            let mut next = units;
            next.add(1 << k, u16::from(term.amount));
            if !assign_term_units(terms, ix + 1, next, f) {
                return false;
            }
        }
    }
    true
}
