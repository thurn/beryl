//! Costs to attack (see [CR 508.1d], see [CR 508.1h], see [CR 508.1i], see [CR 508.1j]), as
//! "Creatures can't attack you unless their controller pays {2} for each creature they control
//! that's attacking you." makes one: which picks of the attack declaration can still be paid
//! for, and the payment once the attackers are chosen and tapped.
//!
//! The total is fixed once the attackers are chosen and is paid after they tap, so a chosen
//! attacker without vigilance can't tap for mana to pay it. An attacker is offered only while
//! the total for it and those already chosen can be paid without the chosen attackers that
//! will tap. Attacking a planeswalker costs nothing, so while the defending player controls
//! one the test is made of attacking the player instead.
//!
//! Every function here is reached only in a game with [`Capability::CombatRequirements`] and
//! is out of line.
//!
//! [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
//! [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
//! [CR 508.1i]: https://yawgatog.com/resources/magic-rules/#R5081i
//! [CR 508.1j]: https://yawgatog.com/resources/magic-rules/#R5081j
//!
//! Implements: none. The combat module declares the rules of declaring attackers.

use beryl_card::ManaCost;

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, Keywords, Types};
use crate::continuous::combat_rules::{attack_cost, times};
use crate::decision::{Decision, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::mana::{self, Funds};
use crate::objects::face_down;
use crate::profile::Payment;
use crate::replacement::event::Sink;
use crate::state::{AttackTarget, Pending, State};
use crate::table::{Capability, GameTable};

/// The cost to attack in force this declaration, with what the active player could pay it
/// with, found once for the many tests of one decision.
#[derive(Clone, Copy, Debug)]
pub(super) struct AttackCosts<L: Layout> {
    /// What one creature attacking the defending player costs: never zero.
    per: ManaCost,
    /// Whether the defending player controls a planeswalker, which can be attacked for free.
    walker: bool,
    /// The active player's pool and untapped sources.
    funds: Funds<L>,
}

impl<L: Layout> AttackCosts<L> {
    /// The cost in force, or `None` if attacking costs nothing.
    #[cold]
    #[inline(never)]
    pub(super) fn of(s: &State<L>, t: &GameTable) -> Result<Option<AttackCosts<L>>, Full> {
        let Some(defending) = defending(s) else {
            return Ok(None);
        };
        let per = attack_cost(s, t, defending)?;
        if per.is_zero() {
            return Ok(None);
        }
        Ok(Some(AttackCosts {
            per,
            walker: controls_planeswalker(s, defending),
            funds: Funds::of(s, t, s.turn().active())?,
        }))
    }

    /// Whether a requirement to attack is waived for every creature (see [CR 508.1d]): "If a
    /// creature can't attack unless a player pays a cost, that player is not required to pay
    /// that cost, even if attacking with that creature would increase the number of
    /// requirements being obeyed." While the defending player controls a planeswalker, each
    /// creature can attack it without paying, so the requirements stand.
    ///
    /// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
    pub(super) fn waives(&self) -> bool {
        !self.walker
    }

    /// Whether each creature declared from now on would attack the defending player, so pay
    /// the cost: there is no planeswalker to attack instead.
    pub(super) fn new_attackers_pay(&self) -> bool {
        !self.walker
    }

    /// Whether the active player can pay for `n` creatures attacking the defending player when
    /// `attackers` are the creatures attacking: those of them without vigilance will be tapped
    /// before the cost is paid (see [CR 508.1f]): "The active player taps the chosen creatures."
    ///
    /// [CR 508.1f]: https://yawgatog.com/resources/magic-rules/#R5081f
    pub(super) fn pays(
        &self,
        s: &State<L>,
        t: &GameTable,
        n: u32,
        attackers: Bitboard<L::Slot>,
    ) -> Result<bool, Full> {
        if n == 0 {
            return Ok(true);
        }
        let total = times(self.per, n)?;
        Ok(self
            .funds
            .pays_for(s, t, total, tapping(s, attackers), mana::Paying::NOTHING))
    }
}

/// The defending player: in a two-player game, the one player who isn't active.
fn defending<L: Layout>(s: &State<L>) -> Option<Seat> {
    s.players_in_turn_order(s.defending_players()).next()
}

/// Whether `player` controls a planeswalker, which creatures may attack instead of them
/// ([`super::attack_targets`]).
fn controls_planeswalker<L: Layout>(s: &State<L>, player: Seat) -> bool {
    s.permanents_with_derived()
        .any(|(_, d)| d.controller == player && d.types.contains(Types::PLANESWALKER))
}

/// Of `attackers`, those that will be tapped as they attack: the ones without vigilance
/// (see [CR 702.20b]): "Attacking doesn't cause creatures with vigilance to tap."
///
/// [CR 702.20b]: https://yawgatog.com/resources/magic-rules/#R70220b
pub(super) fn tapping<L: Layout>(s: &State<L>, attackers: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
    attackers.filter(|slot| !s.derived(slot).keywords.contains(Keywords::VIGILANCE))
}

/// Whether the creature in `slot`, if it attacked, could change what pays the cost: it will
/// be tapped and it has a mana ability.
pub(super) fn taps_a_source<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    !s.derived(slot).keywords.contains(Keywords::VIGILANCE)
        && s.abilities(t, slot)
            .any(|(_, ability)| ability.kind() == AbilityKind::Mana)
}

/// The creatures attacking the defending player itself, which pay the cost, rather than a
/// planeswalker.
pub(super) fn attacking_the_player<L: Layout>(s: &State<L>) -> u32 {
    let n = (s.battlefield().iter())
        .filter(|&&slot| {
            matches!(
                s.object(slot).attack_target(),
                Some(AttackTarget::Player(_))
            )
        })
        .count();
    u32::try_from(n).expect("fewer attackers than slots")
}

/// The creatures attacking.
#[inline]
pub(super) fn attacking<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    (s.battlefield().iter())
        .copied()
        .filter(|&slot| s.object(slot).is_attacking())
        .collect()
}

/// Removes attacking the defending player from `targets`, what the creature just chosen may
/// attack, when the cost for it and the others attacking that player can't be paid. It is
/// offered only while the defending player controls a planeswalker: the creature was offered
/// because it could attack that for free.
#[cold]
#[inline(never)]
pub(super) fn keep_payable_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    targets: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    let Some(costs) = AttackCosts::of(s, t)? else {
        return Ok(());
    };
    // The creature just chosen attacks the player until another target is chosen for it.
    if costs.pays(s, t, attacking_the_player(s), attacking(s))? {
        return Ok(());
    }
    let mut kept: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for &opt in targets.as_slice() {
        if opt != Opt::Defender(None) {
            kept.push(opt)?;
        }
    }
    *targets = kept;
    Ok(())
}

/// The total cost to attack for the creatures declared, all now chosen (see [CR 508.1h]): what
/// one creature attacking the defending player costs, once for each.
///
/// [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
#[cold]
#[inline(never)]
fn total<L: Layout>(s: &State<L>, t: &GameTable) -> Result<ManaCost, Full> {
    let Some(defending) = defending(s) else {
        return Ok(ManaCost::NONE);
    };
    let n = attacking_the_player(s);
    if n == 0 {
        return Ok(ManaCost::NONE);
    }
    times(attack_cost(s, t, defending)?, n)
}

/// Pays the total cost to attack, once the attackers have tapped, then announces the attack;
/// or asks how to pay it, and announces the attack once it is paid ([`pay`]).
#[cold]
#[inline(never)]
pub(super) fn pay_then_announce<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let owed = total(s, t)?;
    if !owed.is_zero() && !begin_payment(s, t, sink, owed)? {
        return Ok(());
    }
    super::finish::announce_attack(s, t, sink, attacking(s))
}

/// Begins paying the total cost to attack, `owed`, after the attackers have tapped
/// (see [CR 508.1i]): "If any of the costs require mana, the active player then has a chance
/// to activate mana abilities", and (see [CR 508.1j]) "Once the player has enough mana in their
/// mana pool, they pay all costs in any order." One plan is paid without asking; several, or
/// paying by hand, are asked one answer at a time. Returns whether all of it is paid.
#[cold]
#[inline(never)]
fn begin_payment<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    owed: ManaCost,
) -> Result<bool, Full> {
    let player = s.turn().active();
    if s.profile().payment != Payment::Manual {
        let greedy = s.profile().payment == Payment::Greedy;
        let plans = face_down::plans(s, t, sink, player, owed, greedy)?;
        debug_assert!(!plans.is_empty(), "an attack offered is paid for");
        if let [plan] = plans.as_slice() {
            face_down::pay_plan(s, t, sink, player, owed, plan)?;
            return Ok(true);
        }
    }
    s.ask(Pending::PayingToAttack { remaining: owed });
    Ok(false)
}

/// The question of paying what is left of the cost to attack, `remaining`: a plan, or under
/// manual payment the next source (§10).
#[cold]
#[inline(never)]
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    remaining: ManaCost,
) -> Result<Decision<L>, Full> {
    face_down::special_action_payment(s, t, sink, s.turn().active(), remaining)
}

/// Applies an answer to the question of paying the cost to attack, then finishes the
/// declaration once all of it is paid.
#[cold]
#[inline(never)]
pub(crate) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    remaining: ManaCost,
    option: Opt<L>,
) -> Result<(), Full> {
    let player = s.turn().active();
    let remaining = face_down::pay_mana(s, t, sink, player, remaining, option)?;
    if remaining.is_zero() {
        s.settle_question();
        sink.together();
        let declared = super::finish::announce_attack(s, t, sink, attacking(s));
        sink.apart();
        declared
    } else {
        s.ask(Pending::PayingToAttack { remaining });
        Ok(())
    }
}

/// Whether some card of the game may make attacking cost something: one test of the
/// capability every combat rule sets, so a game with none asks nothing more. The cost itself
/// is found out of line ([`AttackCosts::of`], [`total`]).
#[inline]
pub(super) fn possible(t: &GameTable) -> bool {
    t.can(Capability::CombatRequirements)
}
