//! What a pool and its sources can pay: the funds a cost is checked against.

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::state::State;
use crate::table::GameTable;

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
                let card = consumed.then(|| s.copiable(slot).key());
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
        })
    }

    /// Whether the player can pay `cost` with none of `exclude`, as [`payable`]. `s` is the
    /// state the funds were found in.
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

    /// The untapped sources but those in `exclude`.
    pub(crate) fn sources(&self, exclude: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
        self.slots & !exclude
    }
}

/// The payability prefilter: whether `player` can pay `cost` at all.
pub(crate) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
) -> Result<bool, Full> {
    Ok(available(s, t, player, exclude)?.pays(cost))
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
