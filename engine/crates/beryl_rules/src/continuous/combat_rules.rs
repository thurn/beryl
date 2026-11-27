//! Combat requirements, restrictions and costs to attack in force (see [CR 508.1c], see
//! [CR 508.1d], see [CR 509.1b], see [CR 509.1c]): the rule changes the declarations of
//! attackers and blockers obey, found on the creature they are about or for the player they
//! apply to. A game whose cards have none asks one test of its table.
//!
//! [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
//! [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
//! [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
//! [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
//!
//! Implements: none. Reads combat rule changes for the declarations that obey them.

use beryl_card::{Governed, ManaCost};

use super::rule_change::{maker_of, object_rules, own_condition_holds};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{RuleKind, Rules, State};
use crate::table::{Capability, GameTable};

/// The combat requirements and restrictions in force about the creature in `slot`
/// ([`RuleKind::OF_A_CREATURE_IN_COMBAT`]), which the declarations of attackers and blockers
/// obey (see [CR 508.1c], see [CR 508.1d], see [CR 509.1b], see [CR 509.1c]): "The defending
/// player checks each creature they control to see whether it's affected by any requirements
/// (effects that say a creature must block, or that it must block if some condition is
/// met)." One test in a game whose cards have none ([`Capability::CombatRequirements`]).
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
/// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
/// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
/// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
#[inline]
pub(crate) fn combat_rules_on<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Rules {
    if !t.can(Capability::CombatRequirements) {
        return Rules::NONE;
    }
    combat_rules_in_force(s, t, slot)
}

/// [`combat_rules_on`] in a game whose cards have a combat rule. A rule is about the creature
/// when one of these makes it:
/// - a static ability of its own whose condition holds and that it still has, about
///   [`beryl_card::Objects::This`];
/// - a static ability of an Aura or Equipment attached to it whose condition holds and that
///   the Aura or Equipment still has, about [`beryl_card::Objects::AttachedTo`];
/// - a resolved effect that started it about the creature, for as long as it lasts
///   ([`State::effect_rules_on`]).
#[cold]
#[inline(never)]
fn combat_rules_in_force<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Rules {
    let kinds = (RuleKind::OF_A_CREATURE_IN_COMBAT.iter())
        .fold(Rules::NONE, |rules, &kind| rules.union(t.rules_of(kind)));
    if kinds.is_empty() {
        return Rules::NONE;
    }
    let made_by = |maker: L::Slot, about: beryl_card::Objects| {
        let made = object_rules(s, t, maker).common(kinds);
        made.iter()
            .filter(|&id| {
                t.rule(id).what.about() == Some(about)
                    && (!s.abilities_may_be_lost() || maker_of(s, t, maker, id).is_some())
                    && own_condition_holds(s, t, maker, id)
            })
            .fold(Rules::NONE, Rules::with)
    };
    let attached = (s.battlefield().iter())
        .filter(|&&permanent| s.object(permanent).attached_to() == Some(slot))
        .fold(Rules::NONE, |rules, &permanent| {
            rules.union(made_by(permanent, beryl_card::Objects::AttachedTo))
        });
    let started = s.effect_rules_on(s.object_ref(slot)).common(kinds);
    made_by(slot, beryl_card::Objects::This)
        .union(attached)
        .union(started)
}

/// The combat rules in force for `player` ([`RuleKind::CombatLimit`] and
/// [`RuleKind::AttackCost`]): how many creatures may attack or block each combat, and what
/// attacking `player` costs (see [CR 508.1c], see [CR 508.1h]): "If any of the chosen creatures
/// require paying costs to attack, or if any optional costs to attack were chosen, the active
/// player determines the total cost to attack." They are kept current with the other rules
/// for players ([`State::rules_for`]). One test in a game whose cards have no combat rule.
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
/// [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
#[inline]
pub(crate) fn combat_rules_for<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Rules {
    if !t.can(Capability::CombatRequirements) {
        return Rules::NONE;
    }
    combat_player_rules(s, t, player)
}

/// [`combat_rules_for`] in a game whose cards have a combat rule.
#[cold]
#[inline(never)]
fn combat_player_rules<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Rules {
    let kinds = t
        .rules_of(RuleKind::CombatLimit)
        .union(t.rules_of(RuleKind::AttackCost));
    s.rules_for(player).common(kinds)
}

/// What one creature attacking `defending` costs its controller under the rule changes in force
/// for `defending` ([`RuleKind::AttackCost`]), as "Creatures can't attack you unless their
/// controller pays {2} for each creature they control that's attacking you." says: the sum
/// over every permanent that makes such a change for them, so two of them cost {4} a creature
/// (see [CR 508.1h]): "If any of the chosen creatures require paying costs to attack, or if any
/// optional costs to attack were chosen, the active player determines the total cost to
/// attack."
/// Zero, after one test, in a game whose cards have no combat rule. [`Full`] if the sum has
/// more of a symbol than a cost holds.
///
/// [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
#[inline]
pub(crate) fn attack_cost<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    defending: Seat,
) -> Result<ManaCost, Full> {
    let in_force = combat_rules_for(s, t, defending).common(t.rules_of(RuleKind::AttackCost));
    if in_force.is_empty() {
        return Ok(ManaCost::NONE);
    }
    attack_cost_of_makers(s, t, defending, in_force)
}

/// [`attack_cost`] while a cost to attack `defending` is in force: the permanents that make
/// one of `in_force` for them, each counted once for each such rule it makes. A rule change
/// is numbered once for the game, so the permanents are counted, not the rules.
#[cold]
#[inline(never)]
fn attack_cost_of_makers<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    defending: Seat,
    in_force: Rules,
) -> Result<ManaCost, Full> {
    let lost = s.any_abilities_lost();
    let mut total = ManaCost::NONE;
    for (maker, d) in s.permanents_with_derived() {
        for id in object_rules(s, t, maker).common(in_force).iter() {
            let rule = t.rule(id);
            let Governed::AttackCost(cost) = rule.what else {
                unreachable!("a rule change of the kind that makes attacking cost mana");
            };
            if crate::filter::is_for(&rule.players, defending, d.controller)
                && (!lost || maker_of(s, t, maker, id).is_some())
                && own_condition_holds(s, t, maker, id)
            {
                total = add_checked(total, cost)?;
            }
        }
    }
    Ok(total)
}

/// `cost` paid `n` times: what `n` creatures attacking the same player cost. [`Full`] if the
/// total has more of a symbol than a cost holds.
pub(crate) fn times(cost: ManaCost, n: u32) -> Result<ManaCost, Full> {
    (0..n).try_fold(ManaCost::NONE, |sum, _| add_checked(sum, cost))
}

/// The two costs together, or [`Full`] where a count would not fit: a cost never saturates.
fn add_checked(a: ManaCost, b: ManaCost) -> Result<ManaCost, Full> {
    let fits = |x: u8, y: u8| x.checked_add(y).ok_or(Full);
    let mut sum = a;
    sum.generic = fits(a.generic, b.generic)?;
    for (i, symbol) in sum.symbols.iter_mut().enumerate() {
        *symbol = fits(a.symbols[i], b.symbols[i])?;
    }
    for (colors, n) in b.hybrid.pairs() {
        let pair = beryl_card::Hybrid::pair(colors).expect("a pair of a hybrid cost");
        let total = a.hybrid.count(pair) + n;
        if total > beryl_card::Hybrid::MOST {
            return Err(Full);
        }
        sum.hybrid = sum.hybrid.with_count(pair, total);
    }
    Ok(sum)
}
