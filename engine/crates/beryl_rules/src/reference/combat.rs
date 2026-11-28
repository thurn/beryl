//! Enumerating reference for combat requirements and restrictions (see [CR 508.1c], see
//! [CR 508.1d], see [CR 509.1b], see [CR 509.1c], see [CR 702.111b]).
//!
//! For boards of at most six creatures a side, enumerates every complete declaration from first
//! principles, keeps the legal ones that obey the most requirements, and checks that the picks
//! offered by `combat::attack_decision` and `combat::block_decision` match them. It shares no
//! code with `combat::requirements`, which finds the most requirements by a pruned search. It
//! does read the same inputs as that search: the rules about each creature and player
//! (`combat_rules_on`, `combat_rules_for`), whether one creature can block another
//! (`combat::can_block`, `rule_change::cant_block`), and the cost to attack (`attack_cost`). In a
//! game whose cards have priced mana abilities or change mana abilities, as Mana Reflection
//! doubles what a land makes, whether the cost can be paid is read from every sequence of mana
//! abilities (`sequences::reference_sequences_with`) rather than one unit per source. A
//! mistake in one of those readers is not caught here; `tests/rules.rs` tests them directly.
//!
//! [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
//! [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
//! [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
//! [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
//! [CR 702.111b]: https://yawgatog.com/resources/magic-rules/#R702111b
//!
//! Implements: none. Reference enumeration for combat requirements and restrictions.

use super::check;
use super::{reference_pays, reference_units, sequences};
use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::card::{Keywords, Types};
use crate::combat;
use crate::continuous::combat_rules::{attack_cost, combat_rules_for, combat_rules_on, times};
use crate::continuous::rule_change;
use crate::decision::{Candidates, Opt};
use crate::invariants::{At, Violation};
use crate::layout::{Layout, Queue};
use crate::mana::Paying;
use crate::state::{AttackTarget, Pending, RuleKind, State};
use crate::table::{Capability, GameTable};
use beryl_card::Governed;
use beryl_kernel::decision::interchangeable;

const MAX_CREATURES_PER_SIDE: usize = 6;
const MAX_ENUMERATED: usize = 100_000;

/// Checks the current combat declaration's offered picks against brute-force enumeration when
/// [`Capability::CombatRequirements`] is set and the board has at most six creatures a side.
pub(crate) fn check_combat<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    if !t.can(Capability::CombatRequirements) {
        return Ok(());
    }
    if let Pending::DeclareAttackers { attacker: None } = *s.pending() {
        check_attack_decision(s, t)
    } else if let Pending::DeclareBlockers { blocker } = *s.pending() {
        check_block_decision(s, t, blocker)
    } else {
        Ok(())
    }
}

/// Checks `combat::attack_decision(s, t, None)` against exhaustive enumeration.
fn check_attack_decision<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    let active = s.turn().active();
    let mut already = Vec::new();
    let mut eligible = Vec::new();
    let restricted = rule_change::attacks_restricted(t);
    for (slot, d) in s.permanents_with_derived() {
        if d.controller != active || !d.types.contains(Types::CREATURE) {
            continue;
        }
        let o = s.object(slot);
        if o.is_attacking() {
            already.push(slot);
        } else if !o.is_tapped()
            && (!o.is_summoning_sick() || d.keywords.contains(Keywords::HASTE))
            && (!restricted || !rule_change::cant_attack(s, t, slot))
        {
            eligible.push(slot);
        }
    }
    if already.len() + eligible.len() > MAX_CREATURES_PER_SIDE {
        return Ok(());
    }

    let Some(maximal) = enumerate_attacks(s, t, &already, &eligible) else {
        return Ok(());
    };
    let Ok(decision) = combat::attack_decision(s, t, None) else {
        return Ok(());
    };
    let Candidates::Options(opts) = decision.candidates else {
        return Ok(());
    };

    let done_offered = opts.as_slice().contains(&Opt::Done);
    let done_legal = maximal.iter().any(|mask| *mask == 0);
    check(
        done_offered == done_legal,
        "attack Opt::Done disagrees with enumerating reference",
        Some(At::Player(active)),
    )?;

    let mut offered_slots = Vec::new();
    for &opt in opts.as_slice() {
        if let Opt::Attack { slot, band: None } = opt {
            offered_slots.push(slot);
        }
    }
    for (i, &slot) in eligible.iter().enumerate() {
        let should_offer = maximal.iter().any(|mask| (mask & (1 << i)) != 0);
        let is_offered = offered_slots
            .iter()
            .any(|&cand| cand == slot || interchangeable(s, cand, slot));
        check(
            is_offered == should_offer,
            "attack candidate disagrees with enumerating reference",
            Some(At::Player(active)),
        )?;
    }
    Ok(())
}

/// Enumerates all subsets of `eligible` (as bitmasks) that form a legal attack declaration
/// obeying the maximum possible number of requirements. A declaration whose cost to attack
/// can't be paid is illegal: creatures added attack a planeswalker for free if the defending
/// player controls one, and the defending player otherwise, and the attackers without
/// vigilance can't tap for mana to pay. `None` where the cost or the mana can't be said.
fn enumerate_attacks<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    already: &[L::Slot],
    eligible: &[L::Slot],
) -> Option<Vec<u8>> {
    let active = s.turn().active();
    let limit = combat_rules_for(s, t, active)
        .common(t.rules_of(RuleKind::CombatLimit))
        .iter()
        .filter_map(|id| {
            if let Governed::CombatLimit { attackers, .. } = t.rule(id).what {
                attackers
            } else {
                None
            }
        })
        .min()
        .unwrap_or(u8::MAX) as usize;

    let defending = s.defending_players();
    let has_planeswalker = s
        .permanents_with_derived()
        .any(|(_, d)| d.types.contains(Types::PLANESWALKER) && defending.contains(d.controller));
    let per = match s.players_in_turn_order(defending).next() {
        Some(player) => attack_cost(s, t, player).ok()?,
        None => ManaCost::NONE,
    };
    let waived = !has_planeswalker && !per.is_zero();
    let attacking_player = already
        .iter()
        .filter(|&&slot| {
            matches!(
                s.object(slot).attack_target(),
                Some(AttackTarget::Player(_))
            )
        })
        .count();
    let paid_for = |chosen: &[L::Slot]| -> Option<bool> {
        let added = chosen.len() - already.len();
        let n = attacking_player + if has_planeswalker { 0 } else { added };
        if n == 0 || per.is_zero() {
            return Some(true);
        }
        let total = times(per, u32::try_from(n).ok()?).ok()?;
        pays_to_attack(s, t, chosen, total)
    };

    let is_alone = |slot: L::Slot| {
        combat_rules_on(s, t, slot)
            .common(t.rules_of(RuleKind::Alone))
            .iter()
            .any(|id| matches!(t.rule(id).what, Governed::Alone { attack: true, .. }))
    };
    let reqs = |slot: L::Slot| -> usize {
        if waived {
            return 0;
        }
        combat_rules_on(s, t, slot)
            .common(t.rules_of(RuleKind::MustAttack))
            .iter()
            .count()
    };

    let mut best: Option<usize> = None;
    let mut maximal: Vec<u8> = Vec::new();
    let total_masks = 1usize << eligible.len();
    for mask in 0..total_masks {
        let mut chosen: Vec<L::Slot> = already.to_vec();
        for (i, &slot) in eligible.iter().enumerate() {
            if (mask & (1 << i)) != 0 {
                chosen.push(slot);
            }
        }
        if chosen.len() > limit {
            continue;
        }
        if chosen.len() == 1 && is_alone(chosen[0]) {
            continue;
        }
        if !paid_for(&chosen)? {
            continue;
        }
        let obeyed: usize = chosen.iter().map(|&c| reqs(c)).sum();
        let mask_u8 = u8::try_from(mask).expect("at most 6 creatures");
        match best {
            None => {
                best = Some(obeyed);
                maximal.clear();
                maximal.push(mask_u8);
            }
            Some(b) if obeyed > b => {
                best = Some(obeyed);
                maximal.clear();
                maximal.push(mask_u8);
            }
            Some(b) if obeyed == b => {
                maximal.push(mask_u8);
            }
            _ => {}
        }
    }
    Some(maximal)
}

/// Whether the active player can pay `total` to attack with `chosen`, those of which without
/// vigilance tap before it is paid and so can't tap for mana. In a game whose cards have priced
/// mana abilities or change mana abilities, as Mana Reflection doubles what a land makes, one
/// unit per source can't say what is made, so every sequence of mana abilities is tried.
fn pays_to_attack<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    chosen: &[L::Slot],
    total: ManaCost,
) -> Option<bool> {
    let active = s.turn().active();
    let tapping: Bitboard<L::Slot> = chosen
        .iter()
        .copied()
        .filter(|&slot| !s.derived(slot).keywords.contains(Keywords::VIGILANCE))
        .collect();
    if t.can_either(Capability::PricedMana, Capability::ManaChanges) {
        return sequences::reference_sequences_with(
            s,
            t,
            active,
            total,
            Paying::NOTHING,
            tapping,
            &[],
        );
    }
    let units = reference_units(s, t, active, Paying::NOTHING, tapping)?;
    Some(reference_pays(units.as_slice(), total))
}

/// Checks `combat::block_decision(s, t, chosen)` against exhaustive enumeration.
fn check_block_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    chosen: Option<L::Slot>,
) -> Result<(), Violation> {
    let Some(defender) = s.players_in_turn_order(s.defending_players()).next() else {
        return Ok(());
    };
    let Some(env) = BlockEnv::build(s, t, defender, chosen) else {
        return Ok(());
    };
    let Some(maximal) = env.enumerate(chosen) else {
        return Ok(());
    };

    let Ok(decision) = combat::block_decision(s, t, chosen) else {
        return Ok(());
    };

    if let Some(blocker) = chosen {
        let Candidates::Objects(offered) = decision.candidates else {
            return Ok(());
        };
        let Some(b_idx) = env.blockers.iter().position(|b| b.slot == blocker) else {
            return Ok(());
        };
        for (a_idx, atk) in env.attackers.iter().enumerate() {
            if !combat::can_block(s, t, blocker, atk.slot) || s.object(blocker).blocks(atk.slot) {
                continue;
            }
            let should_offer = maximal.iter().any(|ext| (ext[b_idx] & (1 << a_idx)) != 0);
            let is_offered =
                offered.any(|cand| cand == atk.slot || interchangeable(s, cand, atk.slot));
            check(
                is_offered == should_offer,
                "block target disagrees with enumerating reference",
                Some(At::Player(defender)),
            )?;
        }
    } else {
        let Candidates::Options(opts) = decision.candidates else {
            return Ok(());
        };
        let done_offered = opts.as_slice().contains(&Opt::Done);
        let done_legal = maximal.iter().any(|ext| ext.iter().all(|&m| m == 0));
        check(
            done_offered == done_legal,
            "block Opt::Done disagrees with enumerating reference",
            Some(At::Player(defender)),
        )?;
        let mut offered_slots = Vec::new();
        for &opt in opts.as_slice() {
            if let Opt::Block(slot) = opt {
                offered_slots.push(slot);
            }
        }
        for (b_idx, blk) in env.blockers.iter().enumerate() {
            if blk.options.len() <= 1 {
                continue;
            }
            let should_offer = maximal.iter().any(|ext| ext[b_idx] != 0);
            let is_offered = offered_slots
                .iter()
                .any(|&cand| cand == blk.slot || interchangeable(s, cand, blk.slot));
            check(
                is_offered == should_offer,
                "blocker candidate disagrees with enumerating reference",
                Some(At::Player(defender)),
            )?;
        }
    }
    Ok(())
}

struct RefAttacker<L: Layout> {
    slot: L::Slot,
    mb: usize,
    mbb: usize,
    menace: bool,
    already: usize,
}

struct RefBlocker<L: Layout> {
    slot: L::Slot,
    already_blocking: bool,
    alone: bool,
    /// Legal bitmasks of *additional* attackers this blocker can be assigned to.
    options: Vec<u8>,
}

struct BlockEnv<L: Layout> {
    attackers: Vec<RefAttacker<L>>,
    blockers: Vec<RefBlocker<L>>,
    limit: usize,
}

impl<L: Layout> BlockEnv<L> {
    #[allow(clippy::too_many_lines)]
    fn build(
        s: &State<L>,
        t: &GameTable,
        defender: crate::ids::Seat,
        chosen: Option<L::Slot>,
    ) -> Option<Self> {
        let defending = s.defending_players();
        let must_block = t.rules_of(RuleKind::MustBlock);
        let must_be_blocked = t.rules_of(RuleKind::MustBeBlocked);
        let mut attackers = Vec::new();
        for &slot in s.battlefield() {
            if !matches!(
                s.object(slot).attack_target(),
                Some(AttackTarget::Player(_) | AttackTarget::Planeswalker(_))
            ) {
                continue;
            }
            let rules = combat_rules_on(s, t, slot);
            let already = s
                .battlefield()
                .iter()
                .filter(|&&b| s.object(b).blocks(slot))
                .count();
            attackers.push(RefAttacker {
                slot,
                mb: rules.common(must_block).iter().count(),
                mbb: rules.common(must_be_blocked).iter().count(),
                menace: s.derived(slot).keywords.contains(Keywords::MENACE),
                already,
            });
        }
        if attackers.len() > MAX_CREATURES_PER_SIDE {
            return None;
        }

        let limit = combat_rules_for(s, t, defender)
            .common(t.rules_of(RuleKind::CombatLimit))
            .iter()
            .filter_map(|id| {
                if let Governed::CombatLimit { blockers, .. } = t.rule(id).what {
                    blockers
                } else {
                    None
                }
            })
            .min()
            .unwrap_or(u8::MAX) as usize;

        let mut blockers = Vec::new();
        let mut total_defending_creatures = 0usize;
        let multiple = rule_change::multiple_blocks_possible(t);
        for (slot, d) in s.permanents_with_derived() {
            let o = s.object(slot);
            if defending.contains(d.controller) && d.types.contains(Types::CREATURE) {
                total_defending_creatures += 1;
            }
            let already_blocking = o.is_blocking();
            if !already_blocking
                && (!defending.contains(d.controller)
                    || !d.types.contains(Types::CREATURE)
                    || o.is_tapped()
                    || rule_change::cant_block(s, t, slot))
            {
                continue;
            }
            let alone = combat_rules_on(s, t, slot)
                .common(t.rules_of(RuleKind::Alone))
                .iter()
                .any(|id| matches!(t.rule(id).what, Governed::Alone { block: true, .. }));
            let can_add = !o.is_tapped()
                && !rule_change::cant_block(s, t, slot)
                && (!already_blocking
                    || chosen == Some(slot)
                    || (multiple && rule_change::can_block_any_number(s, t, slot)));
            let any_number = multiple && rule_change::can_block_any_number(s, t, slot);
            let mut legal_mask = 0u8;
            if can_add {
                for (a_idx, atk) in attackers.iter().enumerate() {
                    if !o.blocks(atk.slot) && combat::can_block(s, t, slot, atk.slot) {
                        legal_mask |= 1 << a_idx;
                    }
                }
            }
            let mut options = vec![0u8];
            if any_number {
                for mask in 1u8..(1u8 << attackers.len()) {
                    if (mask & !legal_mask) == 0 {
                        options.push(mask);
                    }
                }
            } else {
                for a_idx in 0..attackers.len() {
                    if (legal_mask & (1 << a_idx)) != 0 {
                        options.push(1 << a_idx);
                    }
                }
            }
            blockers.push(RefBlocker {
                slot,
                already_blocking,
                alone,
                options,
            });
        }
        if total_defending_creatures > MAX_CREATURES_PER_SIDE {
            return None;
        }
        Some(Self {
            attackers,
            blockers,
            limit,
        })
    }

    fn enumerate(&self, must_add_for: Option<L::Slot>) -> Option<Vec<Vec<u8>>> {
        let mut total_combos = 1usize;
        for b in &self.blockers {
            // Combination budget for the debug enumerator; saturating triggers the cap below.
            total_combos = total_combos.saturating_mul(b.options.len());
            if total_combos > MAX_ENUMERATED {
                return None;
            }
        }
        let must_idx =
            must_add_for.and_then(|slot| self.blockers.iter().position(|b| b.slot == slot));
        let mut current = vec![0u8; self.blockers.len()];
        let mut best: Option<usize> = None;
        let mut maximal: Vec<Vec<u8>> = Vec::new();
        self.recurse(0, must_idx, &mut current, &mut best, &mut maximal);
        Some(maximal)
    }

    fn recurse(
        &self,
        idx: usize,
        must_idx: Option<usize>,
        current: &mut [u8],
        best: &mut Option<usize>,
        maximal: &mut Vec<Vec<u8>>,
    ) {
        if idx == self.blockers.len() {
            self.score(current, best, maximal);
            return;
        }
        for &mask in &self.blockers[idx].options {
            if must_idx == Some(idx) && mask == 0 {
                continue;
            }
            current[idx] = mask;
            self.recurse(idx + 1, must_idx, current, best, maximal);
        }
    }

    fn score(&self, current: &[u8], best: &mut Option<usize>, maximal: &mut Vec<Vec<u8>>) {
        let mut total_blockers = 0usize;
        let mut sole_alone = false;
        for (i, b) in self.blockers.iter().enumerate() {
            if b.already_blocking || current[i] != 0 {
                total_blockers += 1;
                if total_blockers == 1 {
                    sole_alone = b.alone;
                }
            }
        }
        if total_blockers > self.limit || (total_blockers == 1 && sole_alone) {
            return;
        }
        let mut obeyed = 0usize;
        for (a_idx, atk) in self.attackers.iter().enumerate() {
            let added = current
                .iter()
                .filter(|&&mask| (mask & (1 << a_idx)) != 0)
                .count();
            let count = atk.already + added;
            if atk.menace && count == 1 {
                return;
            }
            obeyed += atk.mb * count;
            if count >= 1 {
                obeyed += atk.mbb;
            }
        }
        match *best {
            None => {
                *best = Some(obeyed);
                maximal.clear();
                maximal.push(current.to_vec());
            }
            Some(b) if obeyed > b => {
                *best = Some(obeyed);
                maximal.clear();
                maximal.push(current.to_vec());
            }
            Some(b) if obeyed == b => {
                maximal.push(current.to_vec());
            }
            _ => {}
        }
    }
}

/// Test helper: starting from the beginning of a `DeclareAttackers` or `DeclareBlockers` step,
/// verifies that every sequence of offered picks ends in a declaration the enumerating
/// reference calls legal and maximal, and that every such declaration (modulo `collapse`
/// equivalence) is reachable.
#[cfg(test)]
#[allow(clippy::too_many_lines)]
pub(crate) fn verify_all_paths<L: Layout>(s: &State<L>, t: &GameTable) {
    use std::collections::BTreeSet;

    let class_of = |slots: &[L::Slot]| -> Vec<usize> {
        let mut classes = Vec::with_capacity(slots.len());
        let mut reps: Vec<L::Slot> = Vec::new();
        for &slot in slots {
            if let Some(pos) = reps
                .iter()
                .position(|&r| r == slot || interchangeable(s, r, slot))
            {
                classes.push(pos);
            } else {
                classes.push(reps.len());
                reps.push(slot);
            }
        }
        classes
    };

    if let Pending::DeclareAttackers { attacker: None } = *s.pending() {
        let active = s.turn().active();
        let restricted = rule_change::attacks_restricted(t);
        let eligible: Vec<L::Slot> = s
            .permanents_with_derived()
            .filter_map(|(slot, d)| {
                let o = s.object(slot);
                (d.controller == active
                    && d.types.contains(Types::CREATURE)
                    && !o.is_tapped()
                    && (!o.is_summoning_sick() || d.keywords.contains(Keywords::HASTE))
                    && !o.is_attacking()
                    && !(restricted && rule_change::cant_attack(s, t, slot)))
                .then_some(slot)
            })
            .collect();
        let classes = class_of(&eligible);
        let maximal =
            enumerate_attacks(s, t, &[], &eligible).expect("a cost and mana the reference says");
        let mut expected: BTreeSet<Vec<usize>> = BTreeSet::new();
        for mask in maximal {
            let mut decl: Vec<usize> = eligible
                .iter()
                .enumerate()
                .filter(|&(i, _)| (mask & (1 << i)) != 0)
                .map(|(i, _)| classes[i])
                .collect();
            decl.sort_unstable();
            expected.insert(decl);
        }

        let mut reached: BTreeSet<Vec<usize>> = BTreeSet::new();
        let mut stack: Vec<State<L>> = vec![s.clone()];
        while let Some(cur) = stack.pop() {
            check_combat(&cur, t).expect("check_combat failed mid-attack");
            let dec = combat::attack_decision(&cur, t, None).expect("attack_decision");
            let Candidates::Options(opts) = dec.candidates else {
                panic!("expected Options");
            };
            assert!(
                !opts.as_slice().is_empty(),
                "attack decision offered no options (dead end)"
            );
            for &opt in opts.as_slice() {
                if opt == Opt::Done {
                    let mut decl: Vec<usize> = eligible
                        .iter()
                        .enumerate()
                        .filter(|&(_, &slot)| cur.object(slot).is_attacking())
                        .map(|(i, _)| classes[i])
                        .collect();
                    decl.sort_unstable();
                    assert!(
                        expected.contains(&decl),
                        "reached illegal or non-maximal attack declaration: {decl:?}, expected one of {expected:?}"
                    );
                    reached.insert(decl);
                } else if let Opt::Attack { slot, band: None } = opt {
                    let mut next = cur.clone();
                    let mut sink = crate::replacement::event::Sink::default();
                    combat::attack(&mut next, t, &mut sink, opt).expect("apply attack");
                    // Aims the attacker at the last target offered: a planeswalker if the
                    // defending player controls one, which costs nothing to attack.
                    if let Pending::DeclareAttackers {
                        attacker: Some(aiming),
                        ..
                    } = *next.pending()
                    {
                        let aim = combat::attack_decision(&next, t, Some(aiming)).expect("aim");
                        let Candidates::Options(targets) = aim.candidates else {
                            panic!("expected Options");
                        };
                        let target = *targets.as_slice().last().expect("a target");
                        combat::attack(&mut next, t, &mut sink, target).expect("apply defender");
                    }
                    let _ = slot;
                    stack.push(next);
                }
            }
        }
        assert_eq!(
            reached, expected,
            "reachable attack declarations differ from enumerating reference"
        );
    } else if let Pending::DeclareBlockers { blocker: None } = *s.pending() {
        let defender = s
            .players_in_turn_order(s.defending_players())
            .next()
            .expect("defender");
        let env = BlockEnv::build(s, t, defender, None).expect("board within reference limit");
        let maximal = env.enumerate(None).expect("within enumeration cap");
        let b_slots: Vec<L::Slot> = env.blockers.iter().map(|b| b.slot).collect();
        let a_slots: Vec<L::Slot> = env.attackers.iter().map(|a| a.slot).collect();
        let b_classes = class_of(&b_slots);
        let a_classes = class_of(&a_slots);

        let mut expected: BTreeSet<Vec<(usize, usize)>> = BTreeSet::new();
        for ext in maximal {
            let mut pairs = Vec::new();
            for (b_i, &mask) in ext.iter().enumerate() {
                for (a_i, &a_class) in a_classes.iter().enumerate() {
                    if (mask & (1 << a_i)) != 0 {
                        pairs.push((b_classes[b_i], a_class));
                    }
                }
            }
            pairs.sort_unstable();
            expected.insert(pairs);
        }

        let mut reached: BTreeSet<Vec<(usize, usize)>> = BTreeSet::new();
        let mut stack: Vec<State<L>> = vec![s.clone()];
        let mut visited: BTreeSet<Vec<(usize, usize)>> = BTreeSet::new();
        while let Some(cur) = stack.pop() {
            check_combat(&cur, t).expect("check_combat failed mid-block");
            let dec = combat::block_decision(&cur, t, None).expect("block_decision");
            let Candidates::Options(opts) = dec.candidates else {
                panic!("expected Options");
            };
            assert!(
                !opts.as_slice().is_empty(),
                "block decision offered no options (dead end)"
            );
            for &opt in opts.as_slice() {
                if opt == Opt::Done {
                    let mut pairs = Vec::new();
                    for (b_i, &b_slot) in b_slots.iter().enumerate() {
                        for (a_i, &a_slot) in a_slots.iter().enumerate() {
                            if cur.object(b_slot).blocks(a_slot) {
                                pairs.push((b_classes[b_i], a_classes[a_i]));
                            }
                        }
                    }
                    pairs.sort_unstable();
                    assert!(
                        expected.contains(&pairs),
                        "reached illegal or non-maximal block declaration: {pairs:?}, expected one of {expected:?}"
                    );
                    reached.insert(pairs);
                } else if let Opt::Block(b_slot) = opt {
                    let mut picked = cur.clone();
                    let mut sink = crate::replacement::event::Sink::default();
                    combat::choose_blocker(&mut picked, t, &mut sink, opt).expect("choose blocker");
                    check_combat(&picked, t).expect("check_combat failed at block target");
                    let target_dec = combat::block_decision(&picked, t, Some(b_slot))
                        .expect("block target decision");
                    let Candidates::Objects(targets) = target_dec.candidates else {
                        panic!("expected Objects");
                    };
                    assert!(
                        !targets.is_empty(),
                        "chosen blocker offered no targets (dead end)"
                    );
                    for &a_slot in picked.battlefield() {
                        if !targets.contains(a_slot) {
                            continue;
                        }
                        let mut next = picked.clone();
                        combat::block_attacker(&mut next, t, a_slot).expect("block attacker");
                        let mut key = Vec::new();
                        for (b_i, &bs) in b_slots.iter().enumerate() {
                            for (a_i, &aslot) in a_slots.iter().enumerate() {
                                if next.object(bs).blocks(aslot) {
                                    key.push((b_classes[b_i], a_classes[a_i]));
                                }
                            }
                        }
                        key.sort_unstable();
                        if visited.insert(key) {
                            stack.push(next);
                        }
                    }
                }
            }
        }
        assert_eq!(
            reached, expected,
            "reachable block declarations differ from enumerating reference"
        );
    } else {
        panic!("verify_all_paths expects DeclareAttackers or DeclareBlockers");
    }
}
