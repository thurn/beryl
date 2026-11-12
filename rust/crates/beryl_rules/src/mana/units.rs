//! Mana counted by what it can pay for: the units a payment draws on, and whether they pay a
//! cost (architecture §10).

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
            let kinds = some.trailing_zeros() as u8;
            some &= some - 1;
            Some((kinds, self.by_kinds[usize::from(kinds)]))
        })
    }

    pub(super) fn total(&self) -> u32 {
        self.nonempty().map(|(_, n)| u32::from(n)).sum()
    }

    /// Whether these units can pay `cost` (X already announced).
    ///
    /// Colored and colorless symbols need units of their kind
    /// ([CR 107.4a](https://yawgatog.com/resources/magic-rules/#R1074a)): "Colored mana in
    /// costs can be paid only with the appropriate color of mana."; generic symbols take
    /// any. That is a bipartite matching, and by Hall's theorem it exists exactly when every
    /// set of symbol kinds has at least as many units able to pay one of them as it has
    /// symbols, and the units cover the total. A source that makes several mana of one kind
    /// it chooses counts as several independent units, which the cube's colorless rocks are.
    pub(crate) fn pays(&self, cost: ManaCost) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        if self.total() < cost.total(0) {
            return false;
        }
        let present = (0..KINDS)
            .filter(|&k| cost.symbols[k] > 0)
            .fold(0u8, |m, k| m | (1 << k));
        let mut mask = present;
        while mask != 0 {
            let need: u32 = (0..KINDS)
                .filter(|&k| mask & (1 << k) != 0)
                .map(|k| u32::from(cost.symbols[k]))
                .sum();
            let cover: u32 = self
                .nonempty()
                .filter(|&(kinds, _)| kinds & mask != 0)
                .map(|(_, n)| u32::from(n))
                .sum();
            if cover < need {
                return false;
            }
            mask = (mask - 1) & present;
        }
        true
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
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        if self.total < cost.total(0) {
            return false;
        }
        // Only sets of kinds the cost has symbols of can fail the condition: adding a kind
        // with no symbols adds no need.
        let present = (0..KINDS)
            .filter(|&k| cost.symbols[k] > 0)
            .fold(0u8, |m, k| m | (1 << k));
        let groups = || self.kinds[..self.len].iter().zip(&self.units[..self.len]);
        let mut mask = present;
        while mask != 0 {
            let need: u32 = (0..KINDS)
                .filter(|&k| mask & (1 << k) != 0)
                .map(|k| u32::from(cost.symbols[k]))
                .sum();
            let cover: u32 = groups()
                .filter(|&(&kinds, _)| kinds & mask != 0)
                .map(|(_, &n)| u32::from(n))
                .sum();
            if cover < need {
                return false;
            }
            mask = (mask - 1) & present;
        }
        true
    }
}
