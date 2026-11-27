//! Spell-cost rule changes (see [CR 601.2f]): per-decision cache and fallback scan.
//!
//! Implements: none. Computes spell-cost modifications from rule changes.
//!
//! [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f

use beryl_card::{Face, Filter, Governed, ManaCost, SpellCost};
use beryl_kernel::layout::Layout;
use beryl_kernel::rule_changes::{RuleId, RuleKind, Rules};

use super::rule_change::{accepts, in_a_scene, maker_of, merged_rules};
use crate::filter;
use crate::ids::Seat;
use crate::interpret;
use crate::state::State;
use crate::table::{Capability, GameTable};

/// `cost`, the total cost of the spell `face` would be before the rule changes in force
/// ([`crate::cast`]'s `spell_cost`), as those that change what casting it costs `player`
/// leave it, as "Noncreature spells cost {1} more to cast." does. While the game's cards
/// make no such change, one test and `cost` as it was.
#[inline]
pub(crate) fn spell_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    face: &Face,
    cost: ManaCost,
) -> ManaCost {
    let of_kind = t.rules_of(RuleKind::SpellCost);
    if of_kind.is_empty() {
        return cost;
    }
    changed_cost(s, t, of_kind, player, face, cost)
}

/// Active spell-cost rule changes for one player in one state, cached once per decision.
pub(crate) struct SpellCosts {
    any: (u8, u8, u8),
    active: [Option<RuleId>; 8],
    len: u8,
    overflow: bool,
}

impl SpellCosts {
    /// Scans the battlefield once for active [`RuleKind::SpellCost`] rules affecting `player`.
    #[cold]
    #[inline(never)]
    pub(crate) fn of<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Self {
        let rules = t.rules_of(RuleKind::SpellCost);
        let (mut any, mut active, mut len) = ((0u8, 0u8, 0u8), [None; 8], 0usize);
        if rules.is_empty() {
            return SpellCosts {
                any,
                active,
                len: 0,
                overflow: false,
            };
        }
        let (merges, lost) = (t.can(Capability::Merges), s.any_abilities_lost());
        for (slot, d) in s.permanents_with_derived() {
            let mut own = s.definition(slot).rules(t);
            if merges {
                own = own.union(merged_rules(s, t, slot));
            }
            for id in own.common(rules).iter() {
                let rule = t.rule(id);
                if !filter::is_for(&rule.players, player, d.controller)
                    || !cost_rule_holds(s, t, slot, id, lost)
                {
                    continue;
                }
                let Governed::SpellCosts { spells, change } = rule.what else {
                    unreachable!("a rule change about what a spell costs");
                };
                if matches!(spells, Filter::Any) {
                    add_cost_change(change, &mut any.0, &mut any.1, &mut any.2);
                } else if len == active.len() {
                    return SpellCosts {
                        any,
                        active,
                        len: 0,
                        overflow: true,
                    };
                } else {
                    active[len] = Some(id);
                    len += 1;
                }
            }
        }
        SpellCosts {
            any,
            active,
            len: u8::try_from(len).expect("at most 8 active spell-cost rules"),
            overflow: false,
        }
    }

    #[inline]
    pub(crate) fn apply<L: Layout>(
        &self,
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        face: &Face,
        cost: ManaCost,
    ) -> ManaCost {
        if self.overflow {
            return changed_cost(s, t, t.rules_of(RuleKind::SpellCost), player, face, cost);
        }
        let (mut more, mut less, mut floor) = self.any;
        if self.len == 0 && (more | less | floor) == 0 {
            return cost;
        }
        for id in self.active[..usize::from(self.len)].iter().flatten() {
            let Governed::SpellCosts { spells, change } = t.rule(*id).what else {
                unreachable!("a rule change about what a spell costs");
            };
            if accepts(spells, face) {
                add_cost_change(change, &mut more, &mut less, &mut floor);
            }
        }
        apply_cost_changes(cost, more, less, floor)
    }
}

#[inline]
fn add_cost_change(change: SpellCost, more: &mut u8, less: &mut u8, floor: &mut u8) {
    // Spell cost adjustments are bounded by `ManaCost`'s `u8` generic field.
    match change {
        SpellCost::More(n) => *more = more.saturating_add(n),
        SpellCost::Less(n) => *less = less.saturating_add(n),
        SpellCost::AtLeast(n) => *floor = (*floor).max(n),
    }
}

#[inline]
fn apply_cost_changes(mut cost: ManaCost, more: u8, less: u8, floor: u8) -> ManaCost {
    // Generic cost floors at 0 and stays within `ManaCost`'s `u8` vocabulary bound.
    cost.generic = cost.generic.saturating_add(more).saturating_sub(less);
    let short = u32::from(floor).saturating_sub(cost.total(0));
    cost.generic = cost
        .generic
        .saturating_add(u8::try_from(short).unwrap_or(u8::MAX));
    cost
}

#[inline]
fn cost_rule_holds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    id: RuleId,
    lost: bool,
) -> bool {
    let holds = if !lost {
        t.rule_condition(id).is_none_or(|c| {
            interpret::without_a_scene(s, slot, c).unwrap_or_else(|| in_a_scene(s, t, slot, id))
        })
    } else if let Some((index, ability)) = maker_of(s, t, slot, id) {
        ability.condition().is_none_or(|c| {
            interpret::without_a_scene(s, slot, c)
                .unwrap_or_else(|| interpret::static_condition_holds(s, t, slot, index, ability))
        })
    } else {
        return false;
    };
    #[cfg(debug_assertions)]
    if holds && let Some((_, ability)) = maker_of(s, t, slot, id) {
        crate::coverage::hit(ability);
    }
    holds
}

/// [`spell_cost`] while the game's cards make a change of what a spell costs: each
/// permanent whose static ability makes one of `rules` for `player`, whose condition holds
/// and whose filter accepts the spell changes it. "Each opponent" and "you" are read for
/// the permanent's controller (see [CR 109.5]): "For a static ability, this is the current
/// controller of the object it's on." Casting applies these changes in order (see [CR 601.2f]):
/// "The total cost is the mana cost or alternative cost (as determined in rule 601.2b), plus all
/// additional costs and cost increases, and minus all cost reductions. If multiple cost
/// reductions apply, the player may apply them in any order. If the mana component of the
/// total cost is reduced to nothing by cost reduction effects, it is considered to be {0}.
/// It can't be reduced to less than {0}." Then the greatest floor: "Once the total cost is
/// determined, any effects that directly affect the total cost are applied." A reduction leaves
/// colored and hybrid symbols as they are (see [CR 118.7a]): "Effects that reduce a cost by an
/// amount of generic mana affect only the generic mana component of that cost. They can't affect
/// the colored or colorless mana components of that cost."
///
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
/// [CR 118.7a]: https://yawgatog.com/resources/magic-rules/#R1187a
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
#[cold]
#[inline(never)]
fn changed_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    rules: Rules,
    player: Seat,
    face: &Face,
    cost: ManaCost,
) -> ManaCost {
    let mut matching = Rules::NONE;
    for id in rules.iter() {
        let Governed::SpellCosts { spells, .. } = t.rule(id).what else {
            unreachable!("a rule change about what a spell costs");
        };
        if accepts(spells, face) {
            matching = matching.with(id);
        }
    }
    if matching.is_empty() {
        return cost;
    }
    let merges = t.can(Capability::Merges);
    let lost = s.any_abilities_lost();
    let (mut more, mut less, mut floor) = (0u8, 0u8, 0u8);
    for (slot, d) in s.permanents_with_derived() {
        let mut own = s.definition(slot).rules(t);
        if merges {
            own = own.union(merged_rules(s, t, slot));
        }
        for id in own.common(matching).iter() {
            let rule = t.rule(id);
            let Governed::SpellCosts { change, .. } = rule.what else {
                unreachable!("a rule change about what a spell costs");
            };
            if filter::is_for(&rule.players, player, d.controller)
                && cost_rule_holds(s, t, slot, id, lost)
            {
                add_cost_change(change, &mut more, &mut less, &mut floor);
            }
        }
    }
    apply_cost_changes(cost, more, less, floor)
}
