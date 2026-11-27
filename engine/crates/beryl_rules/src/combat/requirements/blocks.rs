//! Blocker branch-and-bound solver for [`super::best`] and [`super::completes`] (see
//! [CR 509.1b], see [CR 509.1c], [CR 702.111b]).
//!
//! [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
//! [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
//! [CR 702.111b]: https://yawgatog.com/resources/magic-rules/#R702111b
//!
//! Implements: none. Blocker search helper for `combat::requirements`.

use super::{block_limit, cant_block_alone};
use crate::bitboard::Bitboard;
use crate::card::{Keywords, Types};
use crate::combat::{can_block, has};
use crate::continuous::combat_rules::combat_rules_on;
use crate::continuous::rule_change;
use crate::engine::collapse;
use crate::layout::Layout;
use crate::state::{AttackTarget, RuleKind, State};
use crate::table::GameTable;

/// Whether finishing the block declaration right now (`Opt::Done`) is legal and obeys at
/// least `goal` requirements.
#[cold]
#[inline(never)]
pub(super) fn can_finish_block<L: Layout>(s: &State<L>, t: &GameTable, goal: u8) -> bool {
    let Some(defender) = s.players_in_turn_order(s.defending_players()).next() else {
        return true;
    };
    let limit = usize::from(block_limit(s, t, defender));
    let mut blocker_count = 0usize;
    let mut sole_alone = false;
    for &slot in s.battlefield() {
        if s.object(slot).is_blocking() {
            blocker_count += 1;
            if blocker_count == 1 {
                sole_alone = cant_block_alone(s, t, slot);
            }
        }
    }
    if blocker_count > limit || (blocker_count == 1 && sole_alone) {
        return false;
    }
    let must_block = t.rules_of(RuleKind::MustBlock);
    let must_be_blocked = t.rules_of(RuleKind::MustBeBlocked);
    let mut obeyed = 0u16;
    for &attacker in s.battlefield() {
        if !is_blockable_attacker(s, attacker) {
            continue;
        }
        let blockers = declared_blockers_count(s, attacker);
        // CR 702.111b: "A creature with menace can't be blocked except by two or more creatures."
        if has(s, attacker, Keywords::MENACE) && blockers == 1 {
            return false;
        }
        let rules = combat_rules_on(s, t, attacker);
        let mb = u16::try_from(rules.common(must_block).iter().count()).unwrap_or(u16::MAX);
        let mbb = u16::try_from(rules.common(must_be_blocked).iter().count()).unwrap_or(u16::MAX);
        obeyed += mb * blockers;
        if blockers >= 1 {
            obeyed += mbb;
        }
    }
    obeyed >= u16::from(goal)
}

/// Whether `slot` is an attacking creature attacking a player or planeswalker.
#[cold]
#[inline(never)]
fn is_blockable_attacker<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    matches!(
        s.object(slot).attack_target(),
        Some(AttackTarget::Player(_) | AttackTarget::Planeswalker(_))
    )
}

/// Number of creatures currently declared as blocking `attacker`.
#[cold]
#[inline(never)]
fn declared_blockers_count<L: Layout>(s: &State<L>, attacker: L::Slot) -> u16 {
    u16::try_from(
        s.battlefield()
            .iter()
            .filter(|&&b| s.object(b).blocks(attacker))
            .count(),
    )
    .unwrap_or(u16::MAX)
}

/// An attacking creature that has a requirement (`MustBlock` / `MustBeBlocked`) or `MENACE`.
struct TrackedAttacker {
    mb: u16,
    mbb: u16,
    menace: bool,
    already: u16,
}

/// A class of interchangeable undeclared blockers that each block at most one attacker.
struct NormalClass {
    tracked: Vec<usize>,
    can_block_plain: bool,
    alone: bool,
    count: u16,
    max_mb: u16,
}

/// A class of interchangeable undeclared blockers with `can_block_any_number`.
struct AnyClass {
    non_menace: Vec<usize>,
    menace: Vec<usize>,
    can_block_plain: bool,
    alone: bool,
    count: u16,
    non_menace_mb: u16,
    menace_mb: u16,
}

/// Branch-and-bound state for the blocker requirements search.
struct BlockSearch {
    attackers: Vec<TrackedAttacker>,
    normal: Vec<NormalClass>,
    any: Vec<AnyClass>,
    limit: u16,
    has_any_alone: bool,
    goal: u16,
    best: Option<u16>,
    /// Upper bound on total `MustBlock` gain from normal classes `idx..`.
    normal_suffix_max: Vec<u16>,
    /// Upper bound on total `MustBlock` gain from any-number classes `idx..`.
    any_suffix_max: Vec<u16>,
}

/// Maximum requirements any legal block declaration extending `s` can obey, or `None` if no
/// legal completion exists.
///
/// - If `must_aim` is `Some(blocker)`, `blocker` must be assigned at least one new attacker
///   (used both for `Pending::DeclareBlockers { blocker: Some(b) }` and to test `Opt::Block(b)`).
/// - If `extra_block` is `Some((blocker, attacker))`, that pair is treated as already declared.
/// - If `goal > 0`, the search stops as soon as it finds a legal completion obeying `>= goal`.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_lines)]
pub(super) fn best_block<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    must_aim: Option<L::Slot>,
    extra_block: Option<(L::Slot, L::Slot)>,
    goal: u8,
) -> Option<u8> {
    if let Some(blocker) = must_aim {
        let mut best: Option<u8> = None;
        let mut legal_attackers = Bitboard::EMPTY;
        for &attacker in s.battlefield() {
            if can_block(s, t, blocker, attacker) && !s.object(blocker).blocks(attacker) {
                legal_attackers.insert(attacker);
            }
        }
        let collapsed = collapse(s, legal_attackers);
        for &attacker in s.battlefield() {
            if !collapsed.contains(attacker) {
                continue;
            }
            if let Some(obeyed) = best_block(s, t, None, Some((blocker, attacker)), goal) {
                best = Some(best.map_or(obeyed, |cur| cur.max(obeyed)));
                if goal > 0 && obeyed >= goal {
                    return best;
                }
            }
        }
        return best;
    }

    let Some(defender) = s.players_in_turn_order(s.defending_players()).next() else {
        return Some(0);
    };
    let limit = u16::from(block_limit(s, t, defender));
    let defending = s.defending_players();
    let must_block = t.rules_of(RuleKind::MustBlock);
    let must_be_blocked = t.rules_of(RuleKind::MustBeBlocked);

    let mut tracked_slots: Vec<L::Slot> = Vec::new();
    let mut tracked_attackers: Vec<TrackedAttacker> = Vec::new();
    let mut plain_attackers: Vec<L::Slot> = Vec::new();

    for &attacker in s.battlefield() {
        if !is_blockable_attacker(s, attacker) {
            continue;
        }
        let rules = combat_rules_on(s, t, attacker);
        let mb = u16::try_from(rules.common(must_block).iter().count()).unwrap_or(u16::MAX);
        let mbb = u16::try_from(rules.common(must_be_blocked).iter().count()).unwrap_or(u16::MAX);
        let menace = has(s, attacker, Keywords::MENACE);
        if mb > 0 || mbb > 0 || menace {
            let mut already = declared_blockers_count(s, attacker);
            if let Some((eb, ea)) = extra_block
                && ea == attacker
                && !s.object(eb).blocks(attacker)
            {
                already += 1;
            }
            tracked_slots.push(attacker);
            tracked_attackers.push(TrackedAttacker {
                mb,
                mbb,
                menace,
                already,
            });
        } else {
            plain_attackers.push(attacker);
        }
    }

    let mut already_blockers_count = 0u16;
    let mut already_alone = false;
    let mut has_any_alone = false;
    let mut base_any_menace = vec![0u16; tracked_attackers.len()];
    let mut normal: Vec<NormalClass> = Vec::new();
    let mut any: Vec<AnyClass> = Vec::new();

    let multiple = rule_change::multiple_blocks_possible(t);
    for (slot, derived) in s.permanents_with_derived() {
        let o = s.object(slot);
        let is_extra_blocker = extra_block.is_some_and(|(eb, _)| eb == slot);
        let is_blocking = o.is_blocking() || is_extra_blocker;
        if is_blocking {
            already_blockers_count += 1;
            let alone = cant_block_alone(s, t, slot);
            if already_blockers_count == 1 {
                already_alone = alone;
            }
            has_any_alone |= alone;
            // An already-blocking creature with `can_block_any_number` can still add blocks
            // without increasing `total_blockers`.
            if multiple
                && !o.is_tapped()
                && rule_change::can_block_any_number(s, t, slot)
                && !rule_change::cant_block(s, t, slot)
            {
                for (i, &atk) in tracked_slots.iter().enumerate() {
                    let already_blocks = o.blocks(atk) || extra_block == Some((slot, atk));
                    if !already_blocks && can_block(s, t, slot, atk) {
                        if tracked_attackers[i].menace {
                            base_any_menace[i] += 1;
                        } else {
                            // Blocking a non-menace tracked attacker is always free and optimal.
                            tracked_attackers[i].already += 1;
                        }
                    }
                }
            }
            continue;
        }
        if !defending.contains(derived.controller)
            || !derived.types.contains(Types::CREATURE)
            || o.is_tapped()
            || rule_change::cant_block(s, t, slot)
        {
            continue;
        }
        let mut tracked: Vec<usize> = Vec::new();
        for (i, &atk) in tracked_slots.iter().enumerate() {
            if can_block(s, t, slot, atk) {
                tracked.push(i);
            }
        }
        let can_block_plain = plain_attackers
            .iter()
            .any(|&atk| can_block(s, t, slot, atk));
        if tracked.is_empty() && !can_block_plain {
            continue;
        }
        let alone = cant_block_alone(s, t, slot);
        has_any_alone |= alone;
        let any_number = multiple && rule_change::can_block_any_number(s, t, slot);
        if any_number {
            let mut non_menace = Vec::new();
            let mut menace = Vec::new();
            for i in tracked {
                if tracked_attackers[i].menace {
                    menace.push(i);
                } else {
                    non_menace.push(i);
                }
            }
            if let Some(existing) = any.iter_mut().find(|c| {
                c.non_menace == non_menace
                    && c.menace == menace
                    && c.can_block_plain == can_block_plain
                    && c.alone == alone
            }) {
                existing.count += 1;
            } else {
                let non_menace_mb = non_menace.iter().map(|&i| tracked_attackers[i].mb).sum();
                let menace_mb = menace.iter().map(|&i| tracked_attackers[i].mb).sum();
                any.push(AnyClass {
                    non_menace,
                    menace,
                    can_block_plain,
                    alone,
                    count: 1,
                    non_menace_mb,
                    menace_mb,
                });
            }
        } else {
            // Order tracked attackers by highest `MustBlock` first for tighter pruning.
            tracked.sort_unstable_by(|&a, &b| {
                tracked_attackers[b]
                    .mb
                    .cmp(&tracked_attackers[a].mb)
                    .then_with(|| tracked_attackers[b].mbb.cmp(&tracked_attackers[a].mbb))
                    .then_with(|| a.cmp(&b))
            });
            if let Some(existing) = normal.iter_mut().find(|c| {
                c.tracked == tracked && c.can_block_plain == can_block_plain && c.alone == alone
            }) {
                existing.count += 1;
            } else {
                let max_mb = tracked
                    .iter()
                    .map(|&i| tracked_attackers[i].mb)
                    .max()
                    .unwrap_or(0);
                normal.push(NormalClass {
                    tracked,
                    can_block_plain,
                    alone,
                    count: 1,
                    max_mb,
                });
            }
        }
    }

    if already_blockers_count > limit {
        return None;
    }

    // Sort normal classes with highest `max_mb` first so good solutions are found early.
    normal.sort_unstable_by_key(|a| std::cmp::Reverse(a.max_mb));

    let mut normal_suffix_max = vec![0u16; normal.len() + 1];
    for i in (0..normal.len()).rev() {
        normal_suffix_max[i] = normal_suffix_max[i + 1] + normal[i].count * normal[i].max_mb;
    }
    let mut any_suffix_max = vec![0u16; any.len() + 1];
    for i in (0..any.len()).rev() {
        any_suffix_max[i] =
            any_suffix_max[i + 1] + any[i].count * (any[i].non_menace_mb + any[i].menace_mb);
    }

    let mut counts: Vec<u16> = tracked_attackers.iter().map(|a| a.already).collect();
    let mut any_menace = base_any_menace;
    let mut any_chosen_menace_only: Vec<usize> = Vec::new();
    let mut search = BlockSearch {
        attackers: tracked_attackers,
        normal,
        any,
        limit,
        has_any_alone,
        goal: u16::from(goal),
        best: None,
        normal_suffix_max,
        any_suffix_max,
    };

    search.search_normal(
        0,
        0,
        0,
        already_blockers_count,
        already_alone,
        &mut counts,
        &mut any_menace,
        &mut any_chosen_menace_only,
    );

    search.best.map(|v| u8::try_from(v).unwrap_or(u8::MAX))
}

impl BlockSearch {
    /// Sound upper bound on the maximum requirements achievable from the current partial state.
    #[cold]
    #[inline(never)]
    fn upper_bound(
        &self,
        normal_idx: usize,
        any_idx: usize,
        counts: &[u16],
        any_menace: &[u16],
    ) -> u16 {
        let mut ub = 0u16;
        for (i, a) in self.attackers.iter().enumerate() {
            ub += a.mb * counts[i];
            if a.menace {
                ub += a.mb * any_menace[i];
            }
            ub += a.mbb;
        }
        ub + self.normal_suffix_max[normal_idx] + self.any_suffix_max[any_idx]
    }

    /// Whether the search has already proven `goal` (when `goal > 0`).
    #[cold]
    #[inline(never)]
    fn hit_goal(&self) -> bool {
        self.goal > 0 && self.best.is_some_and(|b| b >= self.goal)
    }

    /// Branch over normal blocker classes, assigning counts of creatures from each class.
    #[cold]
    #[inline(never)]
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn search_normal(
        &mut self,
        class_idx: usize,
        opt_idx: usize,
        left_in_class: u16,
        total_blockers: u16,
        sole_alone: bool,
        counts: &mut [u16],
        any_menace: &mut [u16],
        any_chosen_menace_only: &mut Vec<usize>,
    ) {
        if self.hit_goal() {
            return;
        }
        if class_idx == self.normal.len() {
            self.search_any(
                0,
                total_blockers,
                sole_alone,
                counts,
                any_menace,
                any_chosen_menace_only,
            );
            return;
        }
        let left = if opt_idx == 0 {
            let ub = self.upper_bound(class_idx, 0, counts, any_menace);
            if self.best.is_some_and(|b| ub <= b) || (self.goal > 0 && ub < self.goal) {
                return;
            }
            self.normal[class_idx].count
        } else {
            left_in_class
        };

        let rem_limit = self.limit - total_blockers;
        let tracked_len = self.normal[class_idx].tracked.len();
        let has_plain = self.normal[class_idx].can_block_plain && self.has_any_alone;
        let total_opts = tracked_len + usize::from(has_plain);

        if opt_idx == total_opts || left == 0 || rem_limit == 0 {
            self.search_normal(
                class_idx + 1,
                0,
                0,
                total_blockers,
                sole_alone,
                counts,
                any_menace,
                any_chosen_menace_only,
            );
            return;
        }

        let class_alone = self.normal[class_idx].alone;
        if opt_idx < tracked_len {
            let atk = self.normal[class_idx].tracked[opt_idx];
            let a = &self.attackers[atk];
            let max_k = if a.mb > 0 {
                left.min(rem_limit)
            } else if a.menace {
                let need = 2u16.saturating_sub(counts[atk]);
                let alone_need = if self.has_any_alone && total_blockers < 2 {
                    2 - total_blockers
                } else {
                    0
                };
                left.min(rem_limit).min(need.max(alone_need))
            } else {
                // `mbb > 0`, `!menace`, `mb == 0`: need 1 for `mbb`, or enough to satisfy `Alone`.
                let need = u16::from(counts[atk] == 0);
                let alone_need = if self.has_any_alone && total_blockers < 2 {
                    2 - total_blockers
                } else {
                    0
                };
                left.min(rem_limit).min(need.max(alone_need))
            };
            for k in (0..=max_k).rev() {
                counts[atk] += k;
                let new_total = total_blockers + k;
                let new_sole_alone = if new_total == 1 {
                    if total_blockers == 0 {
                        class_alone
                    } else {
                        sole_alone
                    }
                } else {
                    false
                };
                self.search_normal(
                    class_idx,
                    opt_idx + 1,
                    left - k,
                    new_total,
                    new_sole_alone,
                    counts,
                    any_menace,
                    any_chosen_menace_only,
                );
                counts[atk] -= k;
                if self.hit_goal() {
                    return;
                }
            }
        } else {
            // Plain non-menace attacker option: only useful to raise `total_blockers` to 2.
            let max_k = if total_blockers < 2 {
                left.min(rem_limit).min(2 - total_blockers)
            } else {
                0
            };
            for k in (0..=max_k).rev() {
                let new_total = total_blockers + k;
                let new_sole_alone = if new_total == 1 {
                    if total_blockers == 0 {
                        class_alone
                    } else {
                        sole_alone
                    }
                } else {
                    false
                };
                self.search_normal(
                    class_idx + 1,
                    0,
                    0,
                    new_total,
                    new_sole_alone,
                    counts,
                    any_menace,
                    any_chosen_menace_only,
                );
                if self.hit_goal() {
                    return;
                }
            }
        }
    }

    /// Branch over undeclared `can_block_any_number` classes, choosing how many of each class
    /// participate in blocking.
    #[cold]
    #[inline(never)]
    fn search_any(
        &mut self,
        any_idx: usize,
        total_blockers: u16,
        sole_alone: bool,
        counts: &mut [u16],
        any_menace: &mut [u16],
        any_chosen_menace_only: &mut Vec<usize>,
    ) {
        if self.hit_goal() {
            return;
        }
        if any_idx == self.any.len() {
            self.evaluate_leaf(
                total_blockers,
                sole_alone,
                counts,
                any_menace,
                any_chosen_menace_only,
            );
            return;
        }
        let ub = self.upper_bound(self.normal.len(), any_idx, counts, any_menace);
        if self.best.is_some_and(|b| ub <= b) || (self.goal > 0 && ub < self.goal) {
            return;
        }
        let rem_limit = self.limit - total_blockers;
        let max_k = self.any[any_idx].count.min(rem_limit);
        let class_alone = self.any[any_idx].alone;
        let menace_only =
            self.any[any_idx].non_menace.is_empty() && !self.any[any_idx].can_block_plain;

        for k in (0..=max_k).rev() {
            if k > 0 {
                for &atk in &self.any[any_idx].non_menace {
                    counts[atk] += k;
                }
                for &atk in &self.any[any_idx].menace {
                    any_menace[atk] += k;
                }
                if menace_only {
                    any_chosen_menace_only.push(any_idx);
                }
            }
            let new_total = total_blockers + k;
            let new_sole_alone = if new_total == 1 {
                if total_blockers == 0 {
                    class_alone
                } else {
                    sole_alone
                }
            } else {
                false
            };
            self.search_any(
                any_idx + 1,
                new_total,
                new_sole_alone,
                counts,
                any_menace,
                any_chosen_menace_only,
            );
            if k > 0 {
                if menace_only {
                    any_chosen_menace_only.pop();
                }
                for &atk in &self.any[any_idx].non_menace {
                    counts[atk] -= k;
                }
                for &atk in &self.any[any_idx].menace {
                    any_menace[atk] -= k;
                }
            }
            if self.hit_goal() {
                return;
            }
        }
    }

    /// Evaluates a complete assignment at a leaf, resolving menace attackers for `any_number`
    /// blockers and checking `Alone` and `MENACE` legality.
    #[cold]
    #[inline(never)]
    fn evaluate_leaf(
        &mut self,
        total_blockers: u16,
        sole_alone: bool,
        counts: &[u16],
        any_menace: &[u16],
        any_chosen_menace_only: &[usize],
    ) {
        if total_blockers == 1 && sole_alone {
            return;
        }
        // Any menace-only `any_number` class with `k > 0` must actually block at least one
        // menace attacker (so that attacker must reach >= 2 blockers).
        for &any_idx in any_chosen_menace_only {
            let can_block_any = self.any[any_idx]
                .menace
                .iter()
                .any(|&atk| counts[atk] + any_menace[atk] >= 2);
            if !can_block_any {
                return;
            }
        }
        let mut obeyed = 0u16;
        for (i, a) in self.attackers.iter().enumerate() {
            let final_count = if a.menace {
                let total = counts[i] + any_menace[i];
                if total >= 2 {
                    total
                } else if counts[i] == 1 {
                    // CR 702.111b: stuck at 1 blocker on a menace attacker.
                    return;
                } else {
                    0
                }
            } else {
                counts[i]
            };
            obeyed += a.mb * final_count;
            if final_count >= 1 {
                obeyed += a.mbb;
            }
        }
        self.best = Some(self.best.map_or(obeyed, |cur| cur.max(obeyed)));
    }
}
