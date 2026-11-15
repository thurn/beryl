//! What a pool and its sources can pay: the funds a cost is checked against.

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::State;
use crate::table::{Capability, GameTable};

use super::remembered::{self, Paying};
use super::sources::{available, controlled, source};
use super::{
    CLASSES,
    units::{Groups, Units},
};

/// Everything a player could pay mana with now, found once for the many payability checks of
/// one decision: their pool and untapped sources, as units. What a source makes doesn't
/// depend on which others a check excludes, so [`Funds::pays`] answers as [`payable`] does,
/// taking an excluded source's units back out of the whole.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Funds<L: Layout = Small> {
    player: Seat,
    /// The sources' slots.
    slots: Bitboard<L::Slot>,
    /// What the pool and all the sources make.
    units: Units,
    /// The same, grouped, for the checks that exclude none.
    all: Groups,
    /// Whether a card of the game makes restricted mana ([`Funds::pays_restricted`]).
    restricted: bool,
}

impl<L: Layout> Funds<L> {
    pub(crate) fn of(s: &State<L>, t: &GameTable, player: Seat) -> Result<Funds<L>, Full> {
        let mut units = Units::NONE;
        for (k, &n) in s.player(player).mana().amounts().iter().enumerate() {
            units.add(1 << k, u16::from(n));
        }
        let mut slots = Bitboard::EMPTY;
        // The classes [`Sources::of`] would form, so that more than it holds is `Full` here
        // too.
        let mut classes: L::Queue<_, CLASSES> = Default::default();
        for (slot, d) in controlled(s, player) {
            if let Some((kinds, amount, other, consumed)) = source(s, t, player, slot, d) {
                slots.insert(slot);
                units.add(kinds, u16::from(amount));
                // Sources a plan consumes are a class per card.
                let card = consumed.then(|| s.definition(slot).key());
                let class = (kinds, amount, other, card);
                if other || !classes.iter().any(|&c| c == class) {
                    classes.push(class)?;
                }
            }
        }
        Ok(Funds {
            player,
            slots,
            units,
            all: units.groups(),
            restricted: t.can(Capability::RemembersMana),
        })
    }

    /// Whether the player can pay `cost` with none of `exclude` for no spell, as [`payable`]
    /// does with [`Paying::NOTHING`]. `s` is the state the funds were found in.
    pub(crate) fn pays(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
    ) -> bool {
        debug_assert_eq!(cost.x, 0, "X is announced before payment");
        let excluded = exclude & self.slots;
        if excluded.is_empty() {
            return self.all.pays(cost);
        }
        let mut u = self.units;
        // Each source's units come off its own count, so the order is nothing.
        for slot in s.in_candidate_order(excluded) {
            let (kinds, amount, _, _) =
                source(s, t, self.player, slot, s.derived(slot)).expect("a source");
            u.take(kinds, u16::from(amount));
        }
        u.pays(cost)
    }

    /// Whether the player can pay `cost` with none of `exclude` for what `paying` is for:
    /// [`Funds::pays`], or, where that fails for a spell in a game whose cards make restricted
    /// mana, [`Funds::pays_restricted`]. Restricted mana only adds to what pays.
    #[inline]
    pub(crate) fn pays_for(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> bool {
        self.pays(s, t, cost, exclude)
            || (paying.reads_restrictions(t) && self.pays_restricted(s, t, cost, exclude, paying))
    }

    /// [`Funds::pays`] for a spell, in a game whose cards make restricted mana: a source of
    /// restricted mana the spell's restriction accepts is one unit of the kinds of either of
    /// its mana abilities, and the mana that remembers in the pool that it accepts adds its
    /// units.
    #[cold]
    #[inline(never)]
    pub(crate) fn pays_restricted(
        &self,
        s: &State<L>,
        t: &GameTable,
        cost: ManaCost,
        exclude: Bitboard<L::Slot>,
        paying: Paying<L>,
    ) -> bool {
        let mut u = self.units;
        for slot in s.in_candidate_order(exclude & self.slots) {
            let (kinds, amount, _, _) =
                source(s, t, self.player, slot, s.derived(slot)).expect("a source");
            u.take(kinds, u16::from(amount));
        }
        // More sources of restricted mana than a [`Small`] payment holds: say it may be paid,
        // so that the payment's own scan finds the layout `Full` and the game goes on in
        // `Large`.
        let Ok(restricted) = remembered::restricted_sources(s, t, self.player) else {
            return true;
        };
        for r in restricted.iter() {
            if exclude.contains(r.slot) || !paying.accepts(t, r.restriction, r.chosen_type) {
                continue;
            }
            match source(s, t, self.player, r.slot, s.derived(r.slot)) {
                Some((kinds, 1, _, _)) => {
                    u.take(kinds, 1);
                    u.add(kinds | r.kinds, 1);
                }
                _ => u.add(r.kinds, 1),
            }
        }
        for &(_, kind) in remembered::accepted_entries(s, t, self.player, paying).iter() {
            u.add(1 << kind, 1);
        }
        u.pays(cost)
    }

    /// Whether a card of the game makes restricted mana, so that [`Funds::pays_restricted`]
    /// may pay what [`Funds::pays`] does not.
    #[inline]
    pub(crate) fn may_restrict(&self) -> bool {
        self.restricted
    }

    /// The untapped sources but those in `exclude`.
    pub(crate) fn sources(&self, exclude: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
        self.slots & !exclude
    }
}

/// The payability prefilter: whether `player` can pay `cost` for what `paying` is for at all.
pub(crate) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<bool, Full> {
    Ok(available(s, t, player, exclude, paying)?.pays(cost))
}

/// Pays one unit toward `remaining`: a colored or colorless symbol of its kind if one is
/// left, generic mana otherwise. Paying a symbol of the unit's own kind first is never worse.
pub(super) fn pay_unit(remaining: &mut ManaCost, kind: usize) {
    if remaining.symbols[kind] > 0 {
        remaining.symbols[kind] -= 1;
    } else {
        debug_assert!(remaining.generic > 0);
        remaining.generic -= 1;
    }
}

/// Whether a unit of `kind` can pay part of `remaining`.
pub(super) fn helps(remaining: ManaCost, kind: usize) -> bool {
    remaining.symbols[kind] > 0 || remaining.generic > 0
}
