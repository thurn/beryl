//! What a spell or ability costs (CR 601.2f, CR 601.2h): its casting options, X and the total
//! cost.

use crate::bitboard::Bitboard;
use crate::card::{CastingOption, Castings, Face, ManaCost};
use crate::layout::Layout;
use crate::state::{Casting, Pending, StackEntry, State};
use crate::table::GameTable;

use super::legal::spent_by_cost;
use super::steps::top;
use super::targets::source_of;
use super::timing::entry_ability_of;

/// Where a card is cast from, and what that changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum From {
    /// At priority, from a hand or under a permission.
    Priority,
    /// During a resolution that offers it (CR 608.2g), for free or not.
    Resolution { free: bool },
}

/// The sets of a face's casting options that may be chosen together: any of the additional
/// costs, and at most one alternative cost
/// ([CR 118.9a](https://yawgatog.com/resources/magic-rules/#R1189a)): "Only one alternative
/// cost can be applied to any one spell as it's being cast." Casting without paying the mana
/// cost is itself an alternative cost, and so is a flashback cost, so `alternative` (one of
/// them is paid) excludes the face's own.
pub(super) fn option_sets(face: &Face, alternative: bool) -> impl Iterator<Item = Castings> + '_ {
    Castings::subsets(face).filter(move |&costs| {
        let alternatives = costs.paid(face).filter(|o| o.is_alternative()).count();
        alternatives <= usize::from(!alternative)
    })
}

/// Whether X is announced for this cast, and the least value it may take.
///
/// [CR 107.3b](https://yawgatog.com/resources/magic-rules/#R1073b): an effect that lets a
/// player cast a spell "while paying neither its mana cost nor an alternative cost that
/// includes X, then the only legal choice for X is 0."
pub(super) fn x_range_start(face: &Face, costs: Castings, free: bool) -> Option<u8> {
    let alternative = costs.paid(face).any(CastingOption::is_alternative);
    let in_mana_cost = face.cost.x > 0 && !free && !alternative;
    let mut min = None;
    for o in costs.paid(face) {
        if o.cost().x > 0 {
            let least = match o {
                CastingOption::Kicker { min_x, .. } => min_x,
                _ => 0,
            };
            min = Some(min.map_or(least, |m: u8| m.max(least)));
        }
    }
    if in_mana_cost {
        min = Some(min.unwrap_or(0));
    }
    min
}

/// A spell's total cost ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)):
/// "The total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus
/// all additional costs and cost increases, and minus all cost reductions." `increase` is
/// the generic mana of the cost increases: a permission's, read of the permission before the
/// spell is cast and of the cost changes on the spell after (§6.3).
pub(super) fn spell_cost(
    face: &Face,
    costs: Castings,
    x: u8,
    increase: u8,
    free: bool,
) -> ManaCost {
    let mut cost = if free { ManaCost::NONE } else { face.cost };
    for o in costs.paid(face) {
        if o.is_alternative() {
            cost = o.cost();
        }
    }
    for o in costs.paid(face).filter(|o| !o.is_alternative()) {
        cost = cost.plus(o.cost());
    }
    cost.plus(ManaCost::generic(increase)).with_x(x)
}

/// The total mana cost of the spell or ability being cast, once X is announced.
pub(super) fn total_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    c: &Casting,
) -> ManaCost {
    let face = t.def(entry.card).face(entry.face);
    match entry.ability {
        None => {
            let increase = entry.spell().map_or(0, |spell| s.cost_increase(spell));
            spell_cost(face, entry.costs, entry.x, increase, c.free)
        }
        Some(ability) => {
            let mut cost = face.ability(ability).cost.mana.with_x(entry.x);
            cost.generic -= c.substituted;
            cost
        }
    }
}

/// The total mana cost of the spell or ability being cast were its X announced as `x`; none
/// when nothing is being cast.
/// [CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "The total cost is the
/// mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and
/// cost increases, and minus all cost reductions."
pub fn cost_being_cast<L: Layout>(s: &State<L>, t: &GameTable, x: u8) -> Option<ManaCost> {
    match *s.pending() {
        Pending::Casting(c) => Some(total_cost(s, t, &StackEntry { x, ..top(s) }, &c)),
        _ => None,
    }
}

/// What may be spent paying for the entry being cast, before its other costs are paid: all
/// but a source that pays with itself.
pub(super) fn spent_by_entry<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> Bitboard<L::Slot> {
    match source_of(entry) {
        Some(source) => spent_by_cost(source, entry_ability_of(t, entry)),
        None => Bitboard::EMPTY,
    }
}
