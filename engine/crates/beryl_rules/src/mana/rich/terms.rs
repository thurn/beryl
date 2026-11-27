//! Evaluated terms and concrete outcome enumeration for `mana::rich`.
//!
//! Implements: none. Enumerates mana assignments for rich mana abilities.

use crate::card::{ManaCost, ObjectAbility};

use super::super::KINDS;
use super::super::units::Units;

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
    pub(super) fn each_made(&self, f: &mut impl FnMut([u8; KINDS]) -> bool) -> bool {
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
                total[k] = total[k].saturating_add(term.amount);
            }
            if produced_mask == 0 {
                return f(total);
            }
            assign_each_of(produced_mask, 0, self.type_produced, &mut total, &mut |m| {
                f(*m)
            })
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
    }
}

fn assign_terms(
    terms: &[EvaluatedTerm],
    ix: usize,
    made: &mut [u8; KINDS],
    f: &mut impl FnMut([u8; KINDS]) -> bool,
) -> bool {
    let Some(&term) = terms.get(ix) else {
        return f(*made);
    };
    if term.kinds.is_power_of_two() {
        let k = term.kinds.trailing_zeros() as usize;
        made[k] = made[k].saturating_add(term.amount);
        let cont = assign_terms(terms, ix + 1, made, f);
        made[k] -= term.amount;
        return cont;
    }
    if term.uniform {
        for k in 0..KINDS {
            if term.kinds & (1 << k) != 0 {
                made[k] = made[k].saturating_add(term.amount);
                let cont = assign_terms(terms, ix + 1, made, f);
                made[k] -= term.amount;
                if !cont {
                    return false;
                }
            }
        }
        return true;
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
    f: &mut impl FnMut(&mut [u8; KINDS]) -> bool,
) -> bool {
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
            made[k] = made[k].saturating_add(n);
            let cont = assign_each_of(kinds, k + 1, left - n, made, f);
            made[k] -= n;
            if !cont {
                return false;
            }
        }
        return true;
    }
    true
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
