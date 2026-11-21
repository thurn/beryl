//! The rules the permanents' static abilities change for each player, found after every
//! layer has applied.
//!
//! Implements: CR 613.11

use beryl_kernel::derive::Rederive;

use crate::card::{AbilityIx, AbilityKind};
use crate::definition::Definition;
use crate::filter;
use crate::layout::{Layout, PerSeat};
use crate::state::{Flag, Rules};
use crate::table::{Capability, GameTable};

/// The rules the permanents' static abilities change for each player, by seat ([CR 613.11]): "Some
/// continuous effects affect game rules rather than objects. ... These effects are applied after
/// all other continuous effects have been applied." So control is settled: "each opponent" and
/// "you" in such an ability are read for the permanent's controller as layer 2 left it (see [CR 109.5]:
/// "For a static ability, this is the current controller of the object it's on."). A face-down
/// permanent has no abilities (see [CR 708.2]), which its copiable values answer for.
///
/// Out of line: only a recompute with such a permanent on the battlefield comes here.
///
/// [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
/// [CR 109.5]: https://yawgatog.com/resources/magic-rules/#R1095
/// [CR 708.2]: https://yawgatog.com/resources/magic-rules/#R7082
#[inline(never)]
pub(super) fn player_rules<L: Layout>(s: &Rederive<'_, L>, t: &GameTable) -> PerSeat<Rules> {
    let mut rules = [Rules::NONE; _];
    let merges = t.can(Capability::Merges);
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        let what = s.definition(slot);
        let made = what.player_rules(t);
        if !made.is_empty() {
            player_rules_of(s, t, i, what, false, made, &mut rules);
        }
        if merges && s.object(slot).flags().has(Flag::Copy) {
            merged_player_rules(s, t, i, &mut rules);
        }
    }
    rules
}

/// Adds to `rules` those that the faces of the cards under the merged permanent at
/// battlefield position `i` change, if it is one: a merged permanent has every ability of
/// each card that represents it (see [CR 702.140e]). Out of line: only a copy in a game with a
/// card that merges comes here.
///
/// [CR 702.140e]: https://yawgatog.com/resources/magic-rules/#R702140e
#[inline(never)]
fn merged_player_rules<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    rules: &mut PerSeat<Rules>,
) {
    let slot = s.battlefield()[i];
    for (card, face) in s.copiable(slot).merged_faces(t) {
        let what = Definition::of_face(card, face);
        // The face's rules read whole and narrowed to those for players: asking
        // `player_rules` here too would make the compiler stop inlining it in the scan above.
        let made = what.rules(t).common(t.player_rules());
        if !made.is_empty() {
            player_rules_of(s, t, i, what, true, made, rules);
        }
    }
}

/// Adds to `rules` the rules that `what`, the face of the permanent at battlefield position
/// `i` or, if `merged`, of a card under it, changes for each player, of those in `made`.
// Always inline: the scan above calls it for every permanent with such a rule, as the
// rollout benchmark's recomputes do, and left out of line it costs a call each.
#[allow(clippy::inline_always)]
#[inline(always)]
fn player_rules_of<L: Layout>(
    s: &Rederive<'_, L>,
    t: &GameTable,
    i: usize,
    what: Definition<L>,
    merged: bool,
    made: Rules,
    rules: &mut PerSeat<Rules>,
) {
    let controller = s.rows()[i].controller;
    for (ability, n) in what.face(t).abilities.iter().zip(0..) {
        let (AbilityKind::Static, Some(rule)) = (ability.kind(), ability.rule()) else {
            continue;
        };
        // A permanent that lost the ability changes no rule (CR 613.1f).
        let has = if merged {
            super::merged::has_so_far(s, i)
        } else {
            s.has_ability_so_far(i, AbilityIx::new(n))
        };
        if !has {
            continue;
        }
        // The card table has numbered every rule change its cards name.
        let Some(id) = t.rule_id(rule).filter(|&id| made.contains(id)) else {
            continue;
        };
        // The rule is in force for those players from here: the ability did what it
        // says, as one that changes a permanent does as its layer applies.
        crate::coverage::hit(ability);
        // Each player's set is a union: the order they are met in is nothing.
        for player in s.players().in_seat_order() {
            if filter::is_for(&rule.players, player, controller) {
                rules[player.index()] = rules[player.index()].with(id);
            }
        }
    }
}
