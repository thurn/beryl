//! What a spell or ability costs (CR 601.2f, CR 601.2h): its casting options, X and the total
//! cost.
//!
//! Implements: CR 107.3b, CR 118.9a, CR 702.102c

use crate::bitboard::Bitboard;
use crate::card::{CastingOption, Castings, FACE_DOWN, Face, ManaCost};
use crate::continuous::rule_change;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Casting, PayingObjects, Pending, StackEntry, StackItem, State};
use crate::table::GameTable;

use super::legal::spent_by_cost;
use super::steps::top;
use super::targets::source_of;

/// Where a card is cast from, and what that changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum From {
    /// At priority, from a hand or under a permission.
    Priority,
    /// During a resolution that offers it (see CR 608.2g), for free or not.
    Resolution { free: bool },
}

/// The sets of a face's casting options that may be chosen together: any of the additional
/// costs, and at most one alternative cost
/// ([CR 118.9a](https://yawgatog.com/resources/magic-rules/#R1189a)): "Only one alternative
/// cost can be applied to any one spell as it's being cast." Casting without paying the mana
/// cost is itself an alternative cost, and so is a flashback cost, so `alternative` (one of
/// them is paid) excludes the face's own.
///
/// A prototype card's printed face and its prototyped face list the same options. A cast of
/// the prototyped face chooses [`CastingOption::Prototype`] and a cast of the printed face
/// does not (see [CR 719.3]): "As a player casts a prototype card, the player chooses whether
/// they cast the card normally or cast it as a prototyped spell using the prototype keyword
/// ability".
///
/// [CR 719.3]: https://yawgatog.com/resources/magic-rules/#R7193
pub(super) fn option_sets(face: &Face, alternative: bool) -> impl Iterator<Item = Castings> + '_ {
    Castings::subsets(face).filter(move |&costs| {
        if costs.is_empty() {
            return !face.is_prototyped();
        }
        let mut alternatives = 0;
        let mut prototyped = false;
        for option in costs.paid(face) {
            match option {
                CastingOption::Morph(_) | CastingOption::Fuse => return false,
                CastingOption::Prototype { .. } => prototyped = true,
                CastingOption::Kicker { .. }
                | CastingOption::Offspring(_)
                | CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
                | CastingOption::Alternative { .. }
                | CastingOption::Escape { .. }
                | CastingOption::Mutate { .. } => {}
            }
            alternatives += usize::from(option.is_alternative());
        }
        alternatives <= usize::from(!alternative) && prototyped == face.is_prototyped()
    })
}

/// Whether X is announced for this cast, and the least value it may take.
///
/// [CR 107.3b](https://yawgatog.com/resources/magic-rules/#R1073b): an effect that lets a
/// player cast a spell "while paying neither its mana cost nor an alternative cost that
/// includes X, then the only legal choice for X is 0."
pub(super) fn x_range_start(face: &Face, costs: Castings, free: bool) -> Option<u8> {
    if costs.is_empty() {
        return (face.cost.x > 0 && !free).then_some(0);
    }
    let alternative = costs.paid(face).any(CastingOption::is_alternative);
    let in_mana_cost = face.cost.x > 0 && !free && !alternative;
    let mut min = None;
    for o in costs.paid(face) {
        if o.cost().x > 0 {
            let least = match o {
                CastingOption::Kicker { min_x, .. } => min_x,
                CastingOption::Offspring(_)
                | CastingOption::Impending { .. }
                | CastingOption::Overload { .. }
                | CastingOption::Evoke { .. }
                | CastingOption::Alternative { .. }
                | CastingOption::Escape { .. }
                | CastingOption::Mutate { .. }
                | CastingOption::Morph(_)
                | CastingOption::Fuse
                | CastingOption::Prototype { .. } => 0,
            };
            min = Some(min.map_or(least, |m: u8| m.max(least)));
        }
    }
    if in_mana_cost {
        min = Some(min.unwrap_or(0));
    }
    min
}

/// A spell's total cost ([CR 601.2f]), the one place it is found, in the rule's order: "The
/// total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus all
/// additional costs and cost increases, and minus all cost reductions." Then the effects that
/// set a minimum, which affect the total directly, last. `increase` is the generic mana of a
/// permission's cost increase, read of the permission before the spell is cast and of the
/// cost changes on the spell after; the rule changes in force add theirs
/// ([`rule_change::spell_cost`]). X counts at its announced value.
///
/// The total is locked in ([CR 601.2f]): "Once the total cost is determined, any effects that
/// directly affect the total cost are applied. Then the resulting total cost becomes 'locked
/// in.'" The casting machine reads it only before any of it is paid: the mana is paid first,
/// whole, and a spell's other parts after it read no total.
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn base_spell_cost(
    face: &Face,
    costs: Castings,
    x: u8,
    increase: u8,
    free: bool,
) -> ManaCost {
    let cost = if costs.is_empty() {
        mana_cost(face, free)
    } else {
        with_options(face, costs, free)
    };
    ManaCost {
        generic: cost.generic.saturating_add(increase),
        ..cost
    }
    .with_x(x)
}

#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn spell_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
    costs: Castings,
    x: u8,
    increase: u8,
    free: bool,
) -> ManaCost {
    let cost = base_spell_cost(face, costs, x, increase, free);
    rule_change::spell_cost(s, t, player, face, cost)
}

fn mana_cost(face: &Face, free: bool) -> ManaCost {
    if free {
        ManaCost::NONE
    } else if std::ptr::eq(face, &raw const FACE_DOWN) {
        // See CR 702.37c: "pay {3} rather than pay its mana cost."
        ManaCost::generic(3)
    } else {
        // CR 702.102c: "The total cost of a fused split spell includes the mana cost of
        // each half." Its combined characteristics provide that mana cost.
        face.cost
    }
}

/// The mana or alternative cost plus any chosen additional costs. Keep the option scans out of
/// the common path, which pays the mana cost directly.
#[cold]
#[inline(never)]
fn with_options(face: &Face, costs: Castings, free: bool) -> ManaCost {
    let mut cost = mana_cost(face, free);
    for o in costs.paid(face) {
        if o.is_alternative() {
            cost = o.cost();
        }
    }
    for o in costs.paid(face).filter(|o| !o.is_alternative()) {
        cost = cost.plus(o.cost());
    }
    cost
}

/// The mana still to pay for the spell or ability being cast, once X is announced: its total
/// cost, less the generic mana objects paid for rather than mana ([`Casting::substituted`])
/// and the colored mana creatures paid for convoke ([`Casting::paid_colors`]).
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face of the stack entry being paid for"
)]
pub(super) fn total_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    c: &Casting<L>,
) -> ManaCost {
    let face = t.def(entry.card()).face(entry.face());
    match (entry.item, entry.ability()) {
        (StackItem::Spell { object, costs, .. }, _) => {
            let increase = s.cost_increase(object);
            let mut cost = spell_cost(s, t, c.player, face, costs, entry.x, increase, c.free());
            // Cards delve exiled pay for generic mana of the locked-in total (see CR 702.66a),
            // as do creatures tapped for convoke (see CR 702.51a) and artifacts tapped for
            // improvise (see CR 702.126a), and creatures convoke tapped pay for symbols of
            // their colors.
            cost.generic -= c.substituted;
            if c.paid_colors != 0 {
                for (color, symbols) in cost.symbols.iter_mut().take(5).enumerate() {
                    *symbols -= c.paid(color);
                }
            }
            cost
        }
        (_, Some(ability)) => {
            let a = face.ability(ability);
            let mut cost = a.cost().mana.with_x(entry.x);
            if a.zones().contains(crate::state::Zone::Battlefield)
                && let Some(source) = source_of(entry)
                && rule_change::spends_as_any_color(s, t, c.player, source)
            {
                cost = cost.as_any_color();
            }
            cost.generic -= c.substituted;
            cost
        }
        (_, None) => unreachable!("an ability's entry names its ability"),
    }
}

/// The total mana cost of the spell or ability being cast were its X announced as `x`; none
/// when nothing is being cast.
/// [CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "The total cost is the
/// mana cost or alternative cost (as determined in rule 601.2b), plus all additional costs and
/// cost increases, and minus all cost reductions."
pub fn cost_being_cast<L: Layout>(s: &State<L>, t: &GameTable, x: u8) -> Option<ManaCost> {
    match *s.pending() {
        Pending::Casting(c) => Some(total_cost(
            s,
            t,
            &StackEntry {
                x,
                ..top(s).clone()
            },
            &c,
        )),
        Pending::None
        | Pending::PlayOrDraw { .. }
        | Pending::Mulligans
        | Pending::Discard
        | Pending::LegendRule { .. }
        | Pending::EnterPayingLife { .. }
        | Pending::DeclareAttackers { .. }
        | Pending::DeclareBlockers { .. }
        | Pending::CombatDamage { .. }
        | Pending::ExactCombatDamage(_)
        | Pending::Resolving(_)
        | Pending::StepActions(_)
        | Pending::TurnStart(_)
        | Pending::TurningFaceUp { .. }
        | Pending::OrderEntered { .. }
        | Pending::EnterChoosing { .. }
        | Pending::ManaColor { .. }
        | Pending::ManaType { .. }
        | Pending::Companion { .. }
        | Pending::CompanionPayment { .. }
        | Pending::ArrangePiles { .. } => None,
    }
}

/// What may be spent paying for the entry being cast, before its other costs are paid: all
/// but a source that pays with itself.
pub(super) fn spent_by_entry<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> Bitboard<L::Slot> {
    match source_of(entry) {
        Some(source) => spent_by_cost(source, entry.ability_in(t).expect("an ability's entry")),
        None => Bitboard::EMPTY,
    }
}

/// The position above the last graveyard card chosen toward a cost
/// ([CR 601.2h]): "The player pays the total cost." The byte cursor caches the position
/// for ordinary graveyards. At its limit, recover the position from the chosen cards,
/// which remain in the graveyard until the cost is fully paid.
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub(super) fn graveyard_start<L: Layout>(
    graveyard: &[L::Slot],
    paying: &PayingObjects<L>,
) -> usize {
    if paying.graveyard_from == u8::MAX {
        graveyard
            .iter()
            .rposition(|&slot| paying.chosen.contains(slot))
            .map_or(0, |last| last + 1)
    } else {
        usize::from(paying.graveyard_from)
    }
}

/// Records a card chosen for one payment. Graveyard picks are bottom first
/// ([`graveyard_start`]), so the next pick starts above this one.
pub(super) fn remember_cost_pick<L: Layout>(
    graveyard: &[L::Slot],
    paying: &mut PayingObjects<L>,
    slot: L::Slot,
) {
    if let Some(i) = graveyard.iter().position(|&card| card == slot) {
        paying.graveyard_from = u8::try_from(i + 1).unwrap_or(u8::MAX);
    }
    paying.chosen.insert(slot);
}

#[cfg(test)]
mod tests {
    use beryl_kernel::ids::{LargeSlot, SlotId};

    use super::{PayingObjects, graveyard_start, remember_cost_pick};
    use crate::layout::Large;

    #[test]
    fn graveyard_cursor_reaches_above_the_byte_limit() {
        let graveyard: Vec<_> = (0..300).map(LargeSlot::from_index).collect();
        let mut paying = PayingObjects::<Large>::start(0, 3);
        assert_eq!(graveyard_start(&graveyard, &paying), 0);
        for index in [254, 255, 276] {
            remember_cost_pick(&graveyard, &mut paying, graveyard[index]);
            assert_eq!(graveyard_start(&graveyard, &paying), index + 1);
        }
    }
}
