//! The mana spent on an activated ability's X, which its effect reads as it resolves: Illusionary
//! Mask's "a creature card in your hand whose mana cost could be paid by some amount of, or all
//! of, the mana you spent on {X}".
//!
//! Implements: none. Records the mana an activation spent and compares a mana cost with it.
//!
//! The record is kept only in a game with such a card ([`Capability::SpentOnX`]), only for an
//! ability whose effect reads it, and only in the `Large` layout, so recording it in a `Small`
//! game is [`Full`] and moves the game to `Large` (`beryl_kernel`'s `SpentMana`).

use crate::card::{Behavior, Effect, ManaColor, ManaCost};
use crate::fixed::Full;
use crate::layout::Layout;
use crate::state::{CastKind, Casting, ManaAmounts, StackEntry, State};
use crate::table::{Capability, GameTable};

use super::timing::entry_ability;

/// Whether the effect of `entry`'s ability reads the mana spent on its X.
fn reads_spent<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> bool {
    entry_ability(t, entry).is_some_and(|ability| match ability.body() {
        Behavior::Data(effect) => {
            effect.any(&|instruction| matches!(instruction, Effect::MayCast { within_x: true, .. }))
        }
        Behavior::Custom(_) => false,
    })
}

/// Records `spent`, mana just spent paying the cost of what `c` casts or activates, on its
/// stack entry, if it is an ability whose effect reads it. The ability's mana cost is its
/// {X} alone, so all of it was spent on X. One AND in a game whose cards never read it.
#[inline]
pub(super) fn record<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    c: &Casting<L>,
    spent: ManaAmounts,
) -> Result<(), Full> {
    if !t.can(Capability::SpentOnX) {
        return Ok(());
    }
    record_read(s, t, c, spent)
}

#[cold]
#[inline(never)]
fn record_read<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    c: &Casting<L>,
    spent: ManaAmounts,
) -> Result<(), Full> {
    let entry = s.stack().last().expect("the ability being activated");
    if c.kind != CastKind::Activation || !reads_spent(t, entry) {
        return Ok(());
    }
    for (&color, &amount) in ManaColor::ALL.iter().zip(spent.iter()) {
        if amount > 0 {
            s.record_spent_on_x(color, amount)?;
        }
    }
    Ok(())
}

/// [`record`] of one mana of the kind `color`, spent a unit at a time by a manual payment.
#[inline]
pub(super) fn record_one<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    c: &Casting<L>,
    color: ManaColor,
) -> Result<(), Full> {
    let mut spent = [0; 6];
    spent[color.index()] = 1;
    record(s, t, c, spent)
}

/// Whether `cost`, a card's mana cost, "could be paid by some amount of, or all of" the mana
/// `spent`: each colored or colorless symbol by mana of its own kind, and the generic part by
/// what is left of any kind. Both the amount and the kinds count, as the card's ruling says:
/// with {U}{U} spent, {U}{U}, {1}{U} and {2} could be paid, and {2}{U} and {G} could not. An {X}
/// in the cost is 0, as it is for a card cast without paying its mana cost (see CR 107.3b).
pub(crate) fn pays(cost: ManaCost, spent: ManaAmounts) -> bool {
    let cost = cost.with_x(0);
    if !cost.hybrid.is_empty() {
        // A hybrid symbol by mana of either of its colors (see CR 107.4e).
        return crate::mana::amounts_pay(spent, cost);
    }
    let mut left: u32 = 0;
    for (&need, &have) in cost.symbols.iter().zip(spent.iter()) {
        if have < need {
            return false;
        }
        left += u32::from(have - need);
    }
    left >= u32::from(cost.generic)
}
