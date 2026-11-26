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

use super::rule_change::{maker_of, object_rules, own_condition_holds};
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the combat declarations' requirement search, still to be written, reads it"
    )
)]
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the combat declarations' requirement search, still to be written, reads it"
    )
)]
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the combat declarations' requirement search, still to be written, reads it"
    )
)]
#[inline]
pub(crate) fn combat_rules_for<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Rules {
    if !t.can(Capability::CombatRequirements) {
        return Rules::NONE;
    }
    combat_player_rules(s, t, player)
}

/// [`combat_rules_for`] in a game whose cards have a combat rule.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the combat declarations' requirement search, still to be written, reads it"
    )
)]
#[cold]
#[inline(never)]
fn combat_player_rules<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Rules {
    let kinds = t
        .rules_of(RuleKind::CombatLimit)
        .union(t.rules_of(RuleKind::AttackCost));
    s.rules_for(player).common(kinds)
}
