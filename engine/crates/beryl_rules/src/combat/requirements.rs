//! Attack and block requirements and restrictions ([CR 508.1c], [CR 508.1d], [CR 509.1b],
//! [CR 509.1c], [CR 702.111b]): finds the most requirements any legal completion of the current
//! partial declaration can obey, and filters offered picks to those that can still reach it.
//!
//! Every function here is out of line (`#[cold]` / `#[inline(never)]`) and called only when
//! [`Capability::CombatRequirements`] is set, so games without combat requirements or menace
//! pay one capability test.
//!
//! [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
//! [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
//! [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
//! [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
//! [CR 702.111b]: https://yawgatog.com/resources/magic-rules/#R702111b
//!
//! Implements: CR 702.111b

use super::{banding, eligible_attackers, eligible_blockers};
use crate::bitboard::Bitboard;
use crate::card::Types;
use crate::continuous::combat_rules::{combat_rules_for, combat_rules_on};
use crate::decision::{Candidates, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{Pending, RuleKind, State};
use crate::table::{Capability, GameTable};
use beryl_card::Governed;

mod blocks;
use blocks::{best_block, can_finish_block};

/// The most requirements any legal completion of the current combat declaration can obey
/// ([CR 508.1d], [CR 509.1c]). Returns `0` if no legal completion exists.
///
/// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
/// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
#[cold]
#[inline(never)]
#[allow(dead_code, reason = "part of the requirements solver interface")]
pub(crate) fn best<L: Layout>(s: &State<L>, t: &GameTable) -> u8 {
    if let Pending::DeclareAttackers { .. } = *s.pending() {
        best_attack(s, t, None).unwrap_or(0)
    } else if let Pending::DeclareBlockers { blocker } = *s.pending() {
        best_block(s, t, blocker, None, 0).unwrap_or(0)
    } else {
        0
    }
}

/// Whether a legal completion of the current combat declaration exists that obeys at least
/// `goal` requirements ([CR 508.1d], [CR 509.1c]).
///
/// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
/// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
#[cold]
#[inline(never)]
#[allow(dead_code, reason = "part of the requirements solver interface")]
pub(crate) fn completes<L: Layout>(s: &State<L>, t: &GameTable, goal: u8) -> bool {
    if let Pending::DeclareAttackers { .. } = *s.pending() {
        best_attack(s, t, None).is_some_and(|obeyed| obeyed >= goal)
    } else if let Pending::DeclareBlockers { blocker } = *s.pending() {
        best_block(s, t, blocker, None, goal).is_some_and(|obeyed| obeyed >= goal)
    } else {
        false
    }
}

/// Populates the legal attacker picks (including `Opt::Done` and banding options) when
/// [`Capability::CombatRequirements`] is set ([CR 508.1c], [CR 508.1d]).
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
/// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
#[cold]
#[inline(never)]
pub(super) fn attack_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    list: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let goal = best_attack(s, t, None).unwrap_or(0);
    if can_finish_attack(s, t, goal) {
        list.push(Opt::Done)?;
    }
    let attackers = collapse(s, eligible_attackers::<L, false>(s, t).collect());
    let mut kept = Bitboard::EMPTY;
    for &slot in s.battlefield() {
        if attackers.contains(slot)
            && best_attack(s, t, Some(slot)).is_some_and(|obeyed| obeyed >= goal)
        {
            kept.insert(slot);
        }
    }
    Candidates::<L>::extend_options(list, kept, |out, slot| out.push(Opt::attack(slot)))?;
    if t.can(Capability::Banding) {
        banding::append_attack_options(s, t, kept, list)?;
    }
    Ok(())
}

/// Populates the legal blocker picks (`Opt::Done` and `Opt::Block`) when
/// [`Capability::CombatRequirements`] is set ([CR 509.1b], [CR 509.1c]).
///
/// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
/// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
#[cold]
#[inline(never)]
pub(super) fn block_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    list: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let goal = best_block(s, t, None, None, 0).unwrap_or(0);
    if can_finish_block(s, t, goal) {
        list.push(Opt::Done)?;
    }
    let able = collapse(s, eligible_blockers(s, t).collect());
    let mut kept = Bitboard::EMPTY;
    for &blocker in s.battlefield() {
        if able.contains(blocker)
            && best_block(s, t, Some(blocker), None, goal).is_some_and(|obeyed| obeyed >= goal)
        {
            kept.insert(blocker);
        }
    }
    Candidates::<L>::extend_options(list, kept, |out, slot| out.push(Opt::Block(slot)))
}

/// Filters the attackers `blocker` can block to those that lead to a legal completion obeying
/// the most requirements ([CR 509.1b], [CR 509.1c]).
///
/// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
/// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
#[cold]
#[inline(never)]
pub(super) fn block_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    blocker: L::Slot,
    attackers: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    let collapsed = collapse(s, attackers);
    let goal = best_block(s, t, Some(blocker), None, 0).unwrap_or(0);
    let mut kept = Bitboard::EMPTY;
    for &attacker in s.battlefield() {
        if collapsed.contains(attacker)
            && best_block(s, t, None, Some((blocker, attacker)), goal)
                .is_some_and(|obeyed| obeyed >= goal)
        {
            kept.insert(attacker);
        }
    }
    kept
}

/// The maximum number of attackers `player` may declare this combat ([CR 508.1c]).
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
#[cold]
#[inline(never)]
fn attack_limit<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> u8 {
    let rules = combat_rules_for(s, t, player).common(t.rules_of(RuleKind::CombatLimit));
    rules
        .iter()
        .filter_map(|id| {
            if let Governed::CombatLimit { attackers, .. } = t.rule(id).what {
                attackers
            } else {
                None
            }
        })
        .min()
        .unwrap_or(u8::MAX)
}

/// The maximum number of blockers `player` may declare this combat ([CR 509.1b]).
///
/// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
#[cold]
#[inline(never)]
fn block_limit<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> u8 {
    let rules = combat_rules_for(s, t, player).common(t.rules_of(RuleKind::CombatLimit));
    rules
        .iter()
        .filter_map(|id| {
            if let Governed::CombatLimit { blockers, .. } = t.rule(id).what {
                blockers
            } else {
                None
            }
        })
        .min()
        .unwrap_or(u8::MAX)
}

/// Whether attack requirements are waived because attacking requires paying a cost ([CR 508.1d]):
/// "If a creature can't attack unless a player pays a cost, that player is not required to pay
/// that cost, even if attacking with that creature would increase the number of requirements
/// being obeyed." When the defending player controls a planeswalker, a creature can attack that
/// planeswalker without paying a cost to attack the player.
///
/// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
#[cold]
#[inline(never)]
fn attack_requirements_waived<L: Layout>(s: &State<L>, t: &GameTable) -> bool {
    let cost_rules = t.rules_of(RuleKind::AttackCost);
    if cost_rules.is_empty() {
        return false;
    }
    let defending = s.defending_players();
    let has_planeswalker = s
        .permanents_with_derived()
        .any(|(_, d)| d.types.contains(Types::PLANESWALKER) && defending.contains(d.controller));
    if has_planeswalker {
        return false;
    }
    s.players_in_turn_order(defending)
        .any(|p| !combat_rules_for(s, t, p).common(cost_rules).is_empty())
}

/// Number of `MustAttack` requirements on `slot`.
#[cold]
#[inline(never)]
fn must_attack_count<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> u8 {
    let rules = combat_rules_on(s, t, slot).common(t.rules_of(RuleKind::MustAttack));
    u8::try_from(rules.iter().count()).unwrap_or(u8::MAX)
}

/// Whether `slot` cannot attack alone ([CR 508.1c]).
///
/// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
#[cold]
#[inline(never)]
fn cant_attack_alone<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let rules = combat_rules_on(s, t, slot).common(t.rules_of(RuleKind::Alone));
    rules
        .iter()
        .any(|id| matches!(t.rule(id).what, Governed::Alone { attack: true, .. }))
}

/// Whether `slot` cannot block alone ([CR 509.1b]).
///
/// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
#[cold]
#[inline(never)]
fn cant_block_alone<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    let rules = combat_rules_on(s, t, slot).common(t.rules_of(RuleKind::Alone));
    rules
        .iter()
        .any(|id| matches!(t.rule(id).what, Governed::Alone { block: true, .. }))
}

/// Whether finishing the attack declaration right now (`Opt::Done`) is legal and obeys at
/// least `goal` requirements.
#[cold]
#[inline(never)]
fn can_finish_attack<L: Layout>(s: &State<L>, t: &GameTable, goal: u8) -> bool {
    let limit = usize::from(attack_limit(s, t, s.turn().active()));
    let waived = attack_requirements_waived(s, t);
    let mut count = 0usize;
    let mut sole_alone = false;
    let mut obeyed = 0u16;
    for &slot in s.battlefield() {
        if s.object(slot).is_attacking() {
            count += 1;
            if count == 1 {
                sole_alone = cant_attack_alone(s, t, slot);
            }
            if !waived {
                obeyed += u16::from(must_attack_count(s, t, slot));
            }
        }
    }
    count <= limit && !(count == 1 && sole_alone) && obeyed >= u16::from(goal)
}

/// Maximum requirements any legal attack declaration extending `s` (plus optional `extra`
/// attacker) can obey, or `None` if no legal completion exists.
#[cold]
#[inline(never)]
fn best_attack<L: Layout>(s: &State<L>, t: &GameTable, extra: Option<L::Slot>) -> Option<u8> {
    let limit = usize::from(attack_limit(s, t, s.turn().active()));
    let waived = attack_requirements_waived(s, t);
    let mut already_count = 0usize;
    let mut already_alone = false;
    let mut already_req = 0u16;
    for &slot in s.battlefield() {
        if s.object(slot).is_attacking() || extra == Some(slot) {
            already_count += 1;
            if already_count == 1 {
                already_alone = cant_attack_alone(s, t, slot);
            }
            if !waived {
                already_req += u16::from(must_attack_count(s, t, slot));
            }
        }
    }
    if already_count > limit {
        return None;
    }
    let mut eligible: Vec<(u8, bool)> = Vec::new();
    for slot in eligible_attackers::<L, false>(s, t) {
        if extra == Some(slot) {
            continue;
        }
        let req = if waived {
            0
        } else {
            must_attack_count(s, t, slot)
        };
        eligible.push((req, cant_attack_alone(s, t, slot)));
    }

    let mut best: Option<u16> = None;
    let mut consider = |val: u16| {
        best = Some(best.map_or(val, |cur| cur.max(val)));
    };

    // k = 0 attackers.
    if already_count == 0 {
        consider(0);
    }
    // k = 1 attacker.
    if limit >= 1 {
        if already_count == 1 {
            if !already_alone {
                consider(already_req);
            }
        } else if already_count == 0
            && let Some(max_single) = eligible
                .iter()
                .filter(|&&(_, alone)| !alone)
                .map(|&(req, _)| u16::from(req))
                .max()
        {
            consider(max_single);
        }
    }
    // k >= 2 attackers: no creature attacks alone, so any subset up to `limit` is legal.
    if limit >= 2 && already_count + eligible.len() >= 2 {
        let take = (limit - already_count).min(eligible.len());
        eligible.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
        let added: u16 = eligible.iter().take(take).map(|&(r, _)| u16::from(r)).sum();
        consider(already_req + added);
    }

    best.map(|v| u8::try_from(v).unwrap_or(u8::MAX))
}
