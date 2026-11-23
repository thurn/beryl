//! Checking the lethal assignments that let damage trample over.

use super::{assigns_now, has, power, recipients};
use crate::bitboard::Bitboard;
use crate::card::Keywords;
use crate::layout::{Layout, Queue};
use crate::state::{AttackTarget, DamageDivision, State};

/// Under [CR 702.19b], "When checking for assigned lethal damage, take into account damage already
/// marked on the creature and damage from other creatures that’s being assigned during the
/// same combat damage step". A trampler can leave a blocker short of lethal if another
/// attacker can make up the difference. Honor the choices that already sent excess damage to
/// a player or planeswalker when offering later shares.
///
/// [CR 702.19b]: https://yawgatog.com/resources/magic-rules/#R70219b
pub(super) fn allows<L: Layout>(
    s: &State<L>,
    division: &DamageDivision<L>,
    recipient: L::Slot,
    amount: u8,
) -> bool {
    let mut other_recipients = division.recipients;
    other_recipients.remove(recipient);
    let left = division.remaining - amount;
    let spills = left > 0 && other_recipients.is_empty();
    if spills
        && (!has(s, division.source, Keywords::TRAMPLE)
            || s.object(division.source)
                .attack_target()
                .is_none_or(|to| to == AttackTarget::Removed))
    {
        return false;
    }
    if !s.object(division.source).is_attacking() {
        return true;
    }

    let mut required = if spills {
        recipients(s, division.source)
    } else {
        Bitboard::EMPTY
    };
    for share in division
        .shares
        .iter()
        .filter(|share| share.target.is_none())
    {
        required |= recipients(s, share.source);
    }
    if required.is_empty() {
        return true;
    }
    let needs = s.battlefield().iter().copied().filter_map(|slot| {
        if !required.contains(slot) {
            return None;
        }
        let assigned = division
            .shares
            .iter()
            .filter(|share| share.target == Some(slot))
            .fold(0_u8, |sum, share| sum.saturating_add(share.amount));
        let written = if slot == recipient { amount } else { 0 };
        let missing = i16::from(s.derived(slot).toughness)
            - i16::from(s.object(slot).damage())
            - i16::from(assigned)
            - i16::from(written);
        u8::try_from(missing)
            .ok()
            .filter(|&n| n > 0)
            .map(|n| (slot, n))
    });
    let needs: Vec<_> = needs.collect();
    if needs.is_empty() {
        return true;
    }

    let current = (left, other_recipients);
    let unassigned = s.battlefield().iter().copied().filter_map(|slot| {
        if slot == division.source
            || division.done.contains(slot)
            || !s.object(slot).is_attacking()
            || !assigns_now(s, slot)
        {
            return None;
        }
        Some((power(s, slot), recipients(s, slot) & required))
    });
    let donors: Vec<_> = std::iter::once(current)
        .chain(unassigned)
        .filter(|(amount, blockers)| *amount > 0 && !blockers.is_empty())
        .collect();
    can_assign(&needs, &donors)
}

/// Finds a legal allocation of the power still available to the blockers that must
/// receive lethal damage. Each unit assigned uses only one attacker's power.
fn can_assign<S: crate::ids::SlotId>(needs: &[(S, u8)], donors: &[(u8, Bitboard<S>)]) -> bool {
    if let [(blocker, need)] = needs {
        return donors
            .iter()
            .filter(|(_, targets)| targets.contains(*blocker))
            .map(|(amount, _)| u32::from(*amount))
            .sum::<u32>()
            >= u32::from(*need);
    }

    let mut available: Vec<_> = donors.iter().map(|(amount, _)| *amount).collect();
    let mut assigned = vec![vec![0_u8; needs.len()]; donors.len()];
    let mut visited = vec![false; donors.len()];
    for (target, &(_, mut needed)) in needs.iter().enumerate() {
        while needed > 0 {
            visited.fill(false);
            let added = supply(
                needs,
                donors,
                &mut assigned,
                &mut available,
                &mut visited,
                target,
                needed,
            );
            if added == 0 {
                return false;
            }
            needed -= added;
        }
    }
    true
}

/// Supplies up to `limit` damage to one blocker. If the only attacker who can supply
/// it was already assigned elsewhere, try to replace that assignment with another
/// attacker's damage.
fn supply<S: crate::ids::SlotId>(
    needs: &[(S, u8)],
    donors: &[(u8, Bitboard<S>)],
    assigned: &mut [Vec<u8>],
    available: &mut [u8],
    visited: &mut [bool],
    target: usize,
    limit: u8,
) -> u8 {
    for (donor, &(_, legal)) in donors.iter().enumerate() {
        if visited[donor] || !legal.contains(needs[target].0) {
            continue;
        }
        visited[donor] = true;
        let free = available[donor].min(limit);
        if free > 0 {
            available[donor] -= free;
            assigned[donor][target] += free;
            return free;
        }
        for other in 0..needs.len() {
            let replace = assigned[donor][other].min(limit);
            if other == target || replace == 0 {
                continue;
            }
            let supplied = supply(needs, donors, assigned, available, visited, other, replace);
            if supplied > 0 {
                assigned[donor][other] -= supplied;
                assigned[donor][target] += supplied;
                return supplied;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::can_assign;
    use crate::bitboard::Bitboard;
    use crate::ids::{Slot, SlotId as _};

    #[test]
    fn one_trampler_can_replace_anothers_assignment_to_free_damage() {
        let first = Slot::from_index(0);
        let second = Slot::from_index(1);
        let needs = [(first, 2), (second, 3)];
        let both = Bitboard::single(first) | Bitboard::single(second);
        assert!(can_assign(
            &needs,
            &[(3, both), (2, Bitboard::single(first))]
        ));
        assert!(!can_assign(
            &needs,
            &[(2, both), (3, Bitboard::single(first))]
        ));
    }
}
