//! Life valuation and swing-back damage estimation for the combat planner (`combat.rs`).

use beryl_engine::card::{Keywords, Types};
use beryl_engine::{Layout, Seat, State};

/// What life is worth: each point 2, 10 more each of the last 10, and 15 more each of the
/// last 5, where one trick or loot away from dead.
pub(super) fn life(l: i16) -> i32 {
    let l = i32::from(l.max(0));
    2 * l + 10 * l.min(10) + 15 * l.min(5)
}

/// The damage the opponent's creatures could deal `me` attacking next turn, each of our
/// creatures blocking one of them it could block, the strongest first: our untapped ones, or
/// with `untap`, all of them. With `fragile`, the opponent removes one of our blockers first,
/// as they will when one more blocker is all that keeps us alive.
pub(super) fn swing_back<L: Layout>(s: &State<L>, me: Seat, untap: bool, fragile: bool) -> i32 {
    let creature = |slot: L::Slot| s.derived(slot).types.intersects(Types::CREATURE);
    let flies = |slot: L::Slot, k: Keywords| s.derived(slot).keywords.intersects(k);
    let mut attackers: Vec<(i32, bool)> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&a| s.controller(a) != me && creature(a) && s.derived(a).power > 0)
        .map(|a| (i32::from(s.derived(a).power), flies(a, Keywords::FLYING)))
        .collect();
    attackers.sort_unstable_by(|a, b| b.cmp(a));
    let mut blockers: Vec<bool> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&b| s.controller(b) == me && creature(b) && (untap || !s.object(b).is_tapped()))
        .map(|b| flies(b, Keywords::FLYING.union(Keywords::REACH)))
        .collect();
    if fragile && !blockers.is_empty() {
        // The best of them: one that blocks fliers too, if any.
        let i = blockers.iter().position(|&reach| reach).unwrap_or(0);
        blockers.swap_remove(i);
    }
    let mut damage = 0;
    for (power, flying) in attackers {
        match blockers.iter().position(|&reach| reach || !flying) {
            Some(i) => {
                blockers.swap_remove(i);
            }
            None => damage += power,
        }
    }
    damage
}
