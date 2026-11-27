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

use super::attack_cost::{self, AttackCosts};
use super::{banding, eligible_attackers, eligible_blockers};
use crate::bitboard::Bitboard;
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
        let costs = AttackCosts::of(s, t).ok().flatten();
        best_attack(s, t, None, costs.as_ref())
            .ok()
            .flatten()
            .unwrap_or(0)
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
        let costs = AttackCosts::of(s, t).ok().flatten();
        best_attack(s, t, None, costs.as_ref())
            .ok()
            .flatten()
            .is_some_and(|obeyed| obeyed >= goal)
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
    let costs = AttackCosts::of(s, t)?;
    let costs = costs.as_ref();
    let goal = best_attack(s, t, None, costs)?.unwrap_or(0);
    if can_finish_attack(s, t, goal, costs) {
        list.push(Opt::Done)?;
    }
    let attackers = collapse(s, eligible_attackers::<L, false>(s, t).collect());
    let mut kept = Bitboard::EMPTY;
    for &slot in s.battlefield() {
        if attackers.contains(slot)
            && best_attack(s, t, Some(slot), costs)?.is_some_and(|obeyed| obeyed >= goal)
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
/// least `goal` requirements. Every pick offered kept the cost to attack payable, so the
/// declaration as it stands can be paid for.
#[cold]
#[inline(never)]
fn can_finish_attack<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    goal: u8,
    costs: Option<&AttackCosts<L>>,
) -> bool {
    let limit = usize::from(attack_limit(s, t, s.turn().active()));
    let waived = costs.is_some_and(AttackCosts::waives);
    let mut chosen = Chosen::default();
    for &slot in s.battlefield() {
        if s.object(slot).is_attacking() {
            let required = if waived {
                0
            } else {
                must_attack_count(s, t, slot)
            };
            chosen = chosen.with(required, cant_attack_alone(s, t, slot));
        }
    }
    chosen.count <= limit
        && !(chosen.count == 1 && chosen.sole_alone)
        && chosen.obeyed >= u16::from(goal)
}

/// The creatures a declaration of attackers already has, as the requirements solver counts
/// them: how many, whether the first can't attack alone, and how many requirements they obey.
#[derive(Clone, Copy, Default)]
struct Chosen {
    count: usize,
    sole_alone: bool,
    obeyed: u16,
}

impl Chosen {
    /// The same with one more creature, which obeys `required` requirements by attacking.
    fn with(self, required: u8, alone: bool) -> Chosen {
        Chosen {
            count: self.count + 1,
            sole_alone: if self.count == 0 {
                alone
            } else {
                self.sole_alone
            },
            obeyed: self.obeyed + u16::from(required),
        }
    }
}

/// Maximum requirements any legal attack declaration extending `s` (plus optional `extra`
/// attacker) can obey, or `None` if no legal completion exists. With a cost to attack in force
/// (`costs`), a completion must also be one its controller can pay for
/// ([`best_paid_attack`]).
#[cold]
#[inline(never)]
fn best_attack<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    extra: Option<L::Slot>,
    costs: Option<&AttackCosts<L>>,
) -> Result<Option<u8>, Full> {
    let limit = usize::from(attack_limit(s, t, s.turn().active()));
    let waived = costs.is_some_and(AttackCosts::waives);
    let required = |slot| {
        if waived {
            0
        } else {
            must_attack_count(s, t, slot)
        }
    };
    let mut chosen = Chosen::default();
    let mut attackers = Bitboard::EMPTY;
    for &slot in s.battlefield() {
        if s.object(slot).is_attacking() || extra == Some(slot) {
            chosen = chosen.with(required(slot), cant_attack_alone(s, t, slot));
            attackers.insert(slot);
        }
    }
    let eligible: Vec<L::Slot> = eligible_attackers::<L, false>(s, t)
        .filter(|&slot| extra != Some(slot))
        .collect();
    let best = match costs {
        None => {
            let optional = eligible
                .iter()
                .map(|&slot| (required(slot), cant_attack_alone(s, t, slot)))
                .collect();
            best_of(limit, chosen, optional)
        }
        Some(costs) => {
            let paying = attack_cost::attacking_the_player(s)
                + u32::from(extra.is_some() && costs.new_attackers_pay());
            best_paid_attack(s, t, costs, limit, chosen, attackers, paying, &eligible)?
        }
    };
    Ok(best.map(|v| u8::try_from(v).unwrap_or(u8::MAX)))
}

/// [`best_attack`] with a cost to attack in force: `chosen` and `attackers` are the creatures
/// declared with the candidate, of which `paying` attack the defending player and so pay.
///
/// With no planeswalker to attack instead, every requirement is waived, so the question is
/// only whether a legal completion can be paid for. Adding an attacker never makes one cheaper,
/// so the declaration itself is the completion to test, or with one more attacker if its one
/// creature can't attack alone.
///
/// With a planeswalker, creatures declared from now on attack it for free, but those that
/// will tap take their mana from what pays for the creatures already attacking the player.
/// Each set of such creatures is tried, and the rest are added as freely as with no cost.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn best_paid_attack<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    costs: &AttackCosts<L>,
    limit: usize,
    chosen: Chosen,
    attackers: Bitboard<L::Slot>,
    paying: u32,
    eligible: &[L::Slot],
) -> Result<Option<u16>, Full> {
    if chosen.count > limit {
        return Ok(None);
    }
    if costs.new_attackers_pay() {
        if chosen.count == 0 {
            return Ok(Some(0));
        }
        if !costs.pays(s, t, paying, attackers)? {
            return Ok(None);
        }
        if chosen.count == 1 && chosen.sole_alone {
            if limit < 2 {
                return Ok(None);
            }
            for &other in eligible {
                let mut with_other = attackers;
                with_other.insert(other);
                if costs.pays(s, t, paying + 1, with_other)? {
                    return Ok(Some(0));
                }
            }
            return Ok(None);
        }
        return Ok(Some(0));
    }
    let optional_of = |slots: &mut dyn Iterator<Item = &L::Slot>| -> Vec<(u8, bool)> {
        slots
            .map(|&slot| (must_attack_count(s, t, slot), cant_attack_alone(s, t, slot)))
            .collect()
    };
    if paying == 0 {
        return Ok(best_of(limit, chosen, optional_of(&mut eligible.iter())));
    }
    let (sources, free): (Vec<L::Slot>, Vec<L::Slot>) = eligible
        .iter()
        .partition(|&&slot| attack_cost::taps_a_source(s, t, slot));
    // Each subset of the sources is tried: more than this many is more than the search allows.
    if sources.len() > MOST_SOURCES_TRIED {
        return Err(Full);
    }
    let optional = optional_of(&mut free.iter());
    let mut best = None;
    for mask in 0u32..(1 << sources.len()) {
        let mut with = chosen;
        let mut tapped = attackers;
        for (i, &slot) in sources.iter().enumerate() {
            if mask & (1 << i) != 0 {
                with = with.with(must_attack_count(s, t, slot), cant_attack_alone(s, t, slot));
                tapped.insert(slot);
            }
        }
        if with.count > limit || !costs.pays(s, t, paying, tapped)? {
            continue;
        }
        if let Some(obeyed) = best_of(limit, with, optional.clone()) {
            best = Some(best.map_or(obeyed, |b: u16| b.max(obeyed)));
        }
    }
    Ok(best)
}

/// The most mana-making creatures that may yet attack whose subsets [`best_paid_attack`]
/// tries, one by one, while a planeswalker can be attacked for free.
const MOST_SOURCES_TRIED: usize = 12;

/// The most requirements a legal declaration of `chosen` and some of `optional`, each with the
/// requirements it would obey and whether it can't attack alone, obeys, no more than `limit`
/// attacking: `None` if there is none.
fn best_of(limit: usize, chosen: Chosen, mut optional: Vec<(u8, bool)>) -> Option<u16> {
    if chosen.count > limit {
        return None;
    }
    let mut best: Option<u16> = None;
    let mut consider = |val: u16| {
        best = Some(best.map_or(val, |cur| cur.max(val)));
    };
    // k = 0 attackers.
    if chosen.count == 0 {
        consider(0);
    }
    // k = 1 attacker.
    if limit >= 1 {
        if chosen.count == 1 {
            if !chosen.sole_alone {
                consider(chosen.obeyed);
            }
        } else if chosen.count == 0
            && let Some(max_single) = optional
                .iter()
                .filter(|&&(_, alone)| !alone)
                .map(|&(req, _)| u16::from(req))
                .max()
        {
            consider(max_single);
        }
    }
    // k >= 2 attackers: no creature attacks alone, so any subset up to `limit` is legal.
    if limit >= 2 && chosen.count + optional.len() >= 2 {
        let take = (limit - chosen.count).min(optional.len());
        optional.sort_unstable_by_key(|a| std::cmp::Reverse(a.0));
        let added: u16 = optional.iter().take(take).map(|&(r, _)| u16::from(r)).sum();
        consider(chosen.obeyed + added);
    }
    best
}
