//! Mana counted by what it can pay for: the units a payment draws on, and whether they pay a
//! cost (architecture §10).
//!
//! Implements: CR 107.4a, CR 107.4e

use super::KINDS;
use crate::card::ManaCost;

/// Mana available to pay with, counted by the set of kinds each unit can be: a floating
/// {W} is one unit of `{W}`, an untapped Tundra one unit of `{W, U}`, a Mana Crypt two units
/// of `{C}`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Units {
    by_kinds: [u16; 1 << KINDS],
    /// A bit per set of kinds with a unit, so reading the units skips the empty sets.
    some: u64,
}

impl Default for Units {
    fn default() -> Self {
        Units::NONE
    }
}

impl Units {
    pub(super) const NONE: Units = Units {
        by_kinds: [0; 1 << KINDS],
        some: 0,
    };

    pub(crate) fn add(&mut self, kinds: u8, n: u16) {
        let k = &mut self.by_kinds[usize::from(kinds)];
        *k = k.saturating_add(n);
        if *k > 0 {
            self.some |= 1 << kinds;
        }
    }

    /// Takes back `n` units that [`Units::add`] added. Units never reach the cap that adding
    /// saturates at: a player holds far fewer sources and floating mana.
    pub(super) fn take(&mut self, kinds: u8, n: u16) {
        let k = &mut self.by_kinds[usize::from(kinds)];
        debug_assert!(*k >= n && *k < u16::MAX, "units taken that were added");
        *k -= n;
        if *k == 0 {
            self.some &= !(1 << kinds);
        }
    }

    /// The sets of kinds with units, and how many each has, in order of the sets.
    pub(super) fn nonempty(&self) -> impl Iterator<Item = (u8, u16)> + '_ {
        let mut some = self.some;
        std::iter::from_fn(move || {
            if some == 0 {
                return None;
            }
            let kinds = u8::try_from(some.trailing_zeros()).expect("a bit in a u64");
            some &= some - 1;
            Some((kinds, self.by_kinds[usize::from(kinds)]))
        })
    }

    pub(super) fn total(&self) -> u32 {
        self.nonempty().map(|(_, n)| u32::from(n)).sum()
    }

    /// Whether these units can pay `cost` (X already announced): [`pays`].
    pub(crate) fn pays(&self, cost: ManaCost) -> bool {
        pays(cost, self.total(), |mask| {
            self.nonempty()
                .filter(|&(kinds, _)| kinds & mask != 0)
                .map(|(_, n)| u32::from(n))
                .sum()
        })
    }

    /// The units grouped by their set of kinds, the form [`Units::pays`] checks.
    pub(super) fn groups(&self) -> Groups {
        let mut g = Groups {
            total: self.total(),
            kinds: [0; 1 << KINDS],
            units: [0; 1 << KINDS],
            len: 0,
        };
        for (kinds, n) in self.nonempty() {
            g.kinds[g.len] = kinds;
            g.units[g.len] = n;
            g.len += 1;
        }
        g
    }
}

/// Units grouped by their set of kinds, with their total: what [`Units::pays`] reads, kept by
/// [`super::Funds`] for the many checks of one decision.
#[derive(Clone, Copy, Debug)]
pub(super) struct Groups {
    total: u32,
    /// The first `len` sets of kinds with units, and how many units each has.
    kinds: [u8; 1 << KINDS],
    units: [u16; 1 << KINDS],
    len: usize,
}

impl Groups {
    /// [`Units::pays`].
    pub(super) fn pays(&self, cost: ManaCost) -> bool {
        pays(cost, self.total, |mask| {
            (self.kinds[..self.len].iter().zip(&self.units[..self.len]))
                .filter(|&(&kinds, _)| kinds & mask != 0)
                .map(|(_, &n)| u32::from(n))
                .sum()
        })
    }
}

/// Whether units, `total` of them, of which `cover` gives how many can be one of a set of kinds,
/// pay `cost` (X already announced).
///
/// Colored and colorless symbols need units of their kind
/// ([CR 107.4a](https://yawgatog.com/resources/magic-rules/#R1074a)): "Colored mana in
/// costs can be paid only with the appropriate color of mana."; generic symbols take
/// any. That is a bipartite matching, and by Hall's theorem it exists exactly when every
/// set of symbol kinds has at least as many units able to pay one of them as it has
/// symbols, and the units cover the total. Only sets of kinds the cost has symbols of can
/// fail the condition: adding a kind with no symbols adds no need. A source that makes
/// several mana of one kind it chooses counts as several independent units, which colorless
/// rocks are.
#[inline]
fn pays(cost: ManaCost, total: u32, cover: impl Fn(u8) -> u32) -> bool {
    debug_assert_eq!(cost.x, 0, "X is announced before payment");
    if !cost.hybrid.is_empty() {
        return pays_hybrid(cost, total, cover);
    }
    if total < cost.total(0) {
        return false;
    }
    let present = symbol_kinds(cost);
    let mut mask = present;
    while mask != 0 {
        if cover(mask) < symbols_in(cost, mask) {
            return false;
        }
        mask = (mask - 1) & present;
    }
    true
}

/// The kinds `cost` has colored or colorless symbols of, a bit per kind.
#[inline]
fn symbol_kinds(cost: ManaCost) -> u8 {
    (0..KINDS)
        .filter(|&k| cost.symbols[k] > 0)
        .fold(0u8, |m, k| m | (1 << k))
}

/// How many colored and colorless symbols of `cost` are of the kinds in `mask`.
#[inline]
fn symbols_in(cost: ManaCost, mask: u8) -> u32 {
    (0..KINDS)
        .filter(|&k| mask & (1 << k) != 0)
        .map(|k| u32::from(cost.symbols[k]))
        .sum()
}

/// [`pays`] for a cost with hybrid symbols. A hybrid symbol is paid by a unit of either of its
/// colors ([CR 107.4e]): "A hybrid symbol such as {W/U} can be paid with either white or blue
/// mana". The matching of symbols to units exists exactly when, for every set of kinds, the
/// units that can be one of them are at least as many as the symbols whose kinds all lie in
/// it: the symbols a set of units is the only payer of. Kept apart so that a cost with no
/// hybrid symbol reads none of it.
///
/// [CR 107.4e]: https://yawgatog.com/resources/magic-rules/#R1074e
#[cold]
#[inline(never)]
fn pays_hybrid(cost: ManaCost, total: u32, cover: impl Fn(u8) -> u32) -> bool {
    if total < cost.total(0) {
        return false;
    }
    let present = symbol_kinds(cost) | cost.hybrid.colors();
    let mut mask = present;
    while mask != 0 {
        let need = symbols_in(cost, mask)
            + (cost.hybrid.pairs())
                .filter(|&(pair, _)| pair & !mask == 0)
                .map(|(_, n)| u32::from(n))
                .sum::<u32>();
        if cover(mask) < need {
            return false;
        }
        mask = (mask - 1) & present;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::Units;
    use crate::card::mana;

    const W: u8 = 1;
    const U: u8 = 1 << 1;
    const B: u8 = 1 << 2;
    const G: u8 = 1 << 4;

    fn units(of: &[(u8, u16)]) -> Units {
        let mut u = Units::NONE;
        for &(kinds, n) in of {
            u.add(kinds, n);
        }
        u
    }

    #[test]
    fn a_hybrid_symbol_is_paid_by_either_color() {
        let cost = mana("{1}{W/B}{W/B}");
        assert!(units(&[(W, 2), (G, 1)]).pays(cost));
        assert!(units(&[(B, 2), (G, 1)]).pays(cost));
        assert!(units(&[(W, 1), (B, 1), (G, 1)]).pays(cost));
        assert!(!units(&[(W, 1), (G, 2)]).pays(cost));
        assert!(!units(&[(W, 1), (B, 1)]).pays(cost));
        assert!(units(&[(W, 1), (B, 1)]).groups().pays(mana("{W/B}{W/B}")));
        assert!(!units(&[(W, 1), (U, 2)]).groups().pays(mana("{W/B}{W/B}")));
    }

    #[test]
    fn hybrid_and_colored_symbols_share_units() {
        // {W}{W/U}: two units of W pay it; a W and a U do; two U do not.
        let cost = mana("{W}{W/U}");
        assert!(units(&[(W, 2)]).pays(cost));
        assert!(units(&[(W, 1), (U, 1)]).pays(cost));
        assert!(!units(&[(U, 2)]).pays(cost));
        // Leyline of the Guildpact's four pairs, each sharing green.
        let leyline = mana("{G/W}{G/U}{B/G}{R/G}");
        assert!(units(&[(G, 4)]).pays(leyline));
        assert!(!units(&[(W, 2), (U, 2)]).pays(leyline));
        assert!(units(&[(W, 1), (U, 1), (B, 1), (G, 1)]).pays(leyline));
    }
}
