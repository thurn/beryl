//! Combat damage ([CR 510]): which creatures assign it in a step, how attackers and blockers
//! divide it, and dealing it.
//!
//! [CR 510]: https://yawgatog.com/resources/magic-rules/#R510
//!
//! Implements: CR 510, CR 702.4, CR 702.19b, CR 702.19d

use super::{banding, damage};
use super::{blockers_of, has, in_combat_now};
use crate::bitboard::Bitboard;
use crate::card::{Keywords, Prompt, Purpose};
use crate::continuous::rule_change;
use crate::decision::{Candidates, Choice, Decision, DecisionKind, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::replacement::event::{Recipient, Sink};
use crate::state::{
    AttackTarget, BATTLEFIELD, CombatRole, LethalAssignment, ObjectRef, Pending, State, TurnStep,
};
use crate::table::{Capability, GameTable};

mod exact_damage;

pub(crate) use exact_damage::decision as exact_decision;

const STRIKES_FIRST: Keywords = Keywords::FIRST_STRIKE.union(Keywords::DOUBLE_STRIKE);

/// Whether combat starts with a first-strike damage step ([CR 510.4]): "If at least one
/// attacking or blocking creature has first strike ... or double strike ... as the combat
/// damage step begins, the only creatures that assign combat damage in that step are those
/// with first strike or double strike."
///
/// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
pub(crate) fn first_strike_step<L: Layout>(s: &State<L>) -> bool {
    s.battlefield()
        .iter()
        .any(|&slot| in_combat_now(s, slot) && s.derived(slot).keywords.intersects(STRIKES_FIRST))
}

/// Whether a creature in combat assigns damage now. In the second step, [CR 510.4] names
/// "the remaining attackers and blockers that had neither first strike nor double strike as
/// the first combat damage step began, as well as the remaining attackers and blockers that
/// currently have double strike."
///
/// [CR 702.4c] says "Removing double strike from a creature during the first combat damage step
/// will stop it from assigning combat damage in the second combat damage step."
/// [CR 702.4d] says "Giving double strike to a creature with first strike after it has already
/// dealt combat damage in the first combat damage step will allow the creature to assign
/// combat damage in the second combat damage step."
///
/// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
/// [CR 702.4c]: https://yawgatog.com/resources/magic-rules/#R7024c
/// [CR 702.4d]: https://yawgatog.com/resources/magic-rules/#R7024d
pub(super) fn assigns_now<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    if !in_combat_now(s, slot) {
        return false;
    }
    if s.turn().step() == TurnStep::FirstStrikeDamage {
        s.derived(slot).keywords.intersects(STRIKES_FIRST)
    } else {
        !s.object(slot).struck_first() || has(s, slot, Keywords::DOUBLE_STRIKE)
    }
}

/// The combat damage a creature assigns ([CR 510.1a]): "Each attacking creature and each blocking
/// creature assigns combat damage equal to its power. Creatures that would assign 0 or less damage
/// this way don't assign combat damage at all."
///
/// [CR 510.1a]: https://yawgatog.com/resources/magic-rules/#R5101a
pub(super) fn power<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    use beryl_kernel::Number as _;
    u8::try_from(s.derived(slot).power.to_i64().max(0)).unwrap_or(u8::MAX)
}

/// The damage that would be lethal to a creature: its toughness less the damage already marked on
/// it (see [CR 120.6]): "If the total damage marked on a creature is greater than or equal to its
/// toughness, that creature has been dealt lethal damage". At least 1.
///
/// [CR 120.6]: https://yawgatog.com/resources/magic-rules/#R1206
pub(super) fn lethal_damage<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    use beryl_kernel::Number as _;
    let left = s.derived(slot).toughness.to_i64() - s.object(slot).damage().to_i64();
    u8::try_from(left.clamp(1, 255)).unwrap_or(1)
}

/// What an attacker attacks, as a recipient of its damage: nothing once a planeswalker it
/// attacked has left the battlefield (CR 510.1b).
pub(super) fn attacked<L: Layout>(s: &State<L>, target: AttackTarget<L>) -> Option<Recipient<L>> {
    match target {
        AttackTarget::Player(player) => Some(Recipient::Player(player)),
        AttackTarget::Planeswalker(walker) => Some(Recipient::Permanent(s.object_ref(walker))),
        AttackTarget::Removed => None,
    }
}

/// The player who divides an attacker's or blocker's combat damage.
///
/// Under [CR 510.1c], a blocked creature "assigns its combat damage to those creatures divided as
/// its controller chooses among them." Under [CR 510.1d], a creature blocking multiple attackers
/// "assigns its combat damage divided as its controller chooses among them."
///
/// [CR 510.1c]: https://yawgatog.com/resources/magic-rules/#R5101c
/// [CR 510.1d]: https://yawgatog.com/resources/magic-rules/#R5101d
pub fn assigner<L: Layout>(s: &State<L>, t: &GameTable, creature: L::Slot) -> Seat {
    if t.can(Capability::Banding)
        && let Some(player) = banding::assigner(s, t, creature)
    {
        return player;
    }
    s.controller(creature)
}

/// The next attacker whose damage needs dividing. A lone blocker takes all the damage or,
/// against trample, enough to leave the most damage for the attacked player.
fn next_attacker<L: Layout>(s: &State<L>, done: Bitboard<L::Slot>) -> Option<L::Slot> {
    s.battlefield().iter().copied().find(|&slot| {
        s.object(slot).is_blocked()
            && !done.contains(slot)
            && assigns_now(s, slot)
            && power(s, slot) > 0
            && blockers_of(s, slot).count() > 1
    })
}

/// As [CR 510.1] directs, "First, the active player announces how each attacking creature assigns its
/// combat damage, then the defending player announces how each blocking creature assigns its
/// combat damage." Only games that allow multiple blocks look for blockers that must divide.
///
/// [CR 510.1]: https://yawgatog.com/resources/magic-rules/#R5101
fn next_division<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    done: Bitboard<L::Slot>,
) -> Option<L::Slot> {
    let attacker = next_attacker(s, done);
    if attacker.is_some() || !rule_change::multiple_blocks_possible(t) {
        attacker
    } else {
        next_blocker(s, done)
    }
}

#[cold]
#[inline(never)]
fn next_blocker<L: Layout>(s: &State<L>, done: Bitboard<L::Slot>) -> Option<L::Slot> {
    s.battlefield().iter().copied().find(|&slot| {
        matches!(s.object(slot).combat(), CombatRole::Blocking { attackers, .. } if attackers.is_multiple())
            && !done.contains(slot)
            && assigns_now(s, slot)
            && power(s, slot) > 0
    })
}

pub(super) fn recipients<L: Layout>(s: &State<L>, source: L::Slot) -> Bitboard<L::Slot> {
    match s.object(source).combat() {
        CombatRole::Blocking { attackers, .. } => attackers.iter().collect(),
        CombatRole::Attacking { .. } => blockers_of(s, source),
        CombatRole::None => Bitboard::EMPTY,
    }
}

fn lethal_targets<L: Layout>(source: L::Slot, lethal: &[LethalAssignment<L>]) -> Bitboard<L::Slot> {
    lethal
        .iter()
        .filter_map(|pick| (pick.source == source).then_some(pick.target))
        .collect()
}

/// A division in progress: the recipients picked for lethal damage, those not yet picked, and
/// the damage the source has left once each picked one is assigned lethal damage.
fn division<L: Layout>(
    s: &State<L>,
    source: L::Slot,
    lethal: &[LethalAssignment<L>],
) -> (Bitboard<L::Slot>, Bitboard<L::Slot>, u32) {
    let killed = lethal_targets(source, lethal);
    let spent: u32 = killed.sum(|slot| u32::from(lethal_damage(s, slot)));
    let left = u32::from(power(s, source)) - spent;
    (killed, recipients(s, source) & !killed, left)
}

/// Turns face up each face-down creature that would assign combat damage in this step and is
/// turned face up as it would ([`crate::objects::face_down::before_use`]), before any
/// creature assigns: it then assigns damage equal to its power face up (see CR 510.1a). Out
/// of line: only a game with face-down permanents comes here.
#[cold]
#[inline(never)]
fn turn_up_assigners<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let mut i = 0;
    while let Some(&slot) = s.battlefield().get(i) {
        if assigns_now(s, slot) && power(s, slot) > 0 {
            crate::objects::face_down::before_use(s, t, sink, slot)?;
        }
        i += 1;
    }
    Ok(())
}

/// Records the first-step participants before any damage changes the battlefield. If every
/// assignment is determined, deals the damage immediately (CR 510.1 and CR 510.2).
pub(crate) fn begin_damage<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if t.can(Capability::Morph) {
        turn_up_assigners(s, t, sink)?;
    }
    if s.turn().step() == TurnStep::FirstStrikeDamage {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if assigns_now(s, slot) {
                let permanent = s.permanent_at(slot).expect("a combat damage dealer");
                s.mark_struck_first(permanent);
            }
        }
    }
    if t.can(Capability::Banding) && banding::affects_damage(s, t) {
        return exact_damage::begin(s, t, sink);
    }
    if let Some(source) = next_division(s, t, Bitboard::EMPTY) {
        s.ask(Pending::CombatDamage {
            source,
            done: Bitboard::EMPTY,
            lethal: Default::default(),
        });
        Ok(())
    } else {
        deal(s, t, sink, assign(s, &[])?.as_slice())
    }
}

/// The next lethal recipient, or done if the remaining damage can be assigned without adding
/// one. Picks represent which creatures receive lethal damage from this source. They cannot
/// express different nonlethal splits or overkill, including damage from several sources that
/// adds up to lethal or changes what other creatures can trample over. Those cases need amounts
/// when the split changes the outcome. Combats with banding use exact amounts
/// (see [`exact_damage`]).
///
/// Out of line: only a creature blocked by several creatures, or blocking several, asks this,
/// and inlined it adds to `questions::decision`, which builds every other decision too.
#[cold]
#[inline(never)]
pub(crate) fn division_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    source: L::Slot,
    lethal_picks: &[LethalAssignment<L>],
) -> Result<Decision<L>, Full> {
    let (killed, unpicked, left) = division(s, source, lethal_picks);
    let absorbable: u32 = unpicked.sum(|slot| u32::from(lethal_damage(s, slot)) - 1);
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    if !killed.is_empty() || left <= absorbable {
        list.push(Opt::Done)?;
    }
    let more = unpicked.filter(|slot| u32::from(lethal_damage(s, slot)) <= left);
    Candidates::<L>::extend_options(&mut list, collapse(s, more), |out, slot| {
        out.push(Opt::Lethal(slot))
    })?;
    let player = assigner(s, t, source);
    Ok(Decision {
        player,
        decider: player,
        kind: DecisionKind::DamageDivision,
        candidates: Candidates::Options(list),
        prompt: Prompt::new(Purpose::CombatDamage),
        about: Bitboard::single(source),
        filter: None,
    })
}

/// The smart-stop recommendation for the next pick. Prefer killing more creatures by taking
/// the cheapest lethal assignment first, breaking ties by power. The pick can be silent if it
/// is the only distinct lethal pick or all killable recipients fit.
pub fn preferred_damage<L: Layout>(s: &State<L>, d: &Decision<L>) -> Option<(Choice, bool)> {
    let Pending::CombatDamage {
        source,
        lethal: chosen,
        ..
    } = s.pending()
    else {
        return None;
    };
    let Candidates::Options(options) = &d.candidates else {
        return None;
    };
    let (_, unpicked, left) = division(s, *source, chosen.as_slice());
    let cost: u32 = unpicked.sum(|slot| {
        let cost = u32::from(lethal_damage(s, slot));
        if cost <= left { cost } else { 0 }
    });
    let picks = options.iter().enumerate().filter_map(|(i, option)| {
        let Opt::Lethal(slot) = *option else {
            return None;
        };
        Some((
            Choice(u16::try_from(i).expect("at most one lethal choice per recipient")),
            lethal_damage(s, slot),
            s.derived(slot).power,
        ))
    });
    let unambiguous = cost <= left || picks.clone().count() == 1;
    picks
        .min_by_key(|&(_, cost, power)| (cost, std::cmp::Reverse(power)))
        .map(|(pick, _, _)| (pick, unambiguous))
}

/// Applies a pick and deals the damage once every division is made.
pub(crate) fn divide<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    opt: Opt<L>,
) -> Result<(), Full> {
    if let Pending::ExactCombatDamage(dividing) = s.pending() {
        return exact_damage::answer(s, t, sink, dividing.clone(), opt);
    }
    let Pending::CombatDamage {
        mut source,
        mut done,
        mut lethal,
    } = s.pending().clone()
    else {
        unreachable!("a damage division without its pending state");
    };
    if let Opt::Lethal(target) = opt {
        lethal.push(LethalAssignment { source, target })?;
    } else {
        done.insert(source);
        let Some(next) = next_division(s, t, done) else {
            s.settle_question();
            return deal(s, t, sink, assign(s, lethal.as_slice())?.as_slice());
        };
        source = next;
    }
    s.ask(Pending::CombatDamage {
        source,
        done,
        lethal,
    });
    Ok(())
}

/// One assignment of combat damage. `to` is `None` only in the default a fixed queue fills with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Assignment<L: Layout = Small> {
    source: ObjectRef<L>,
    to: Option<Recipient<L>>,
    amount: u8,
}

/// Deals the combat damage assigned in this step.
///
/// [CR 510.2](https://yawgatog.com/resources/magic-rules/#R5102): "Second, all combat damage that's
/// been assigned is dealt simultaneously." Every assignment is made from the creatures as they are
/// before any damage is dealt, and the events of the damage are kept as one group. Damage to a
/// player is the life they lose (see CR 120.3a), in the one event.
fn deal<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    assigned: &[Assignment<L>],
) -> Result<(), Full> {
    sink.together();
    let dealt = assigned.iter().try_for_each(|a| {
        let Some(to) = a.to else { return Ok(()) };
        damage::deal(&mut Game::new(s, t, sink), a.source, to, a.amount, true).map(drop)
    });
    sink.apart();
    dealt
}

/// The combat damage of this step, including the players' lethal assignments.
fn assign<L: Layout>(
    s: &State<L>,
    lethal: &[LethalAssignment<L>],
) -> Result<L::Queue<Assignment<L>, 128>, Full> {
    let mut assigned: L::Queue<Assignment<L>, 128> = Default::default();
    for &slot in s.battlefield() {
        if !assigns_now(s, slot) {
            continue;
        }
        let amount = power(s, slot);
        if amount == 0 {
            continue;
        }
        let o = s.object(slot);
        let source = s.object_ref(slot);
        let mut push = |to: Recipient<L>, amount: u8| {
            assigned.push(Assignment {
                source,
                to: Some(to),
                amount,
            })
        };
        match o.combat() {
            // CR 510.1d: "If it’s blocking exactly one creature, it assigns all its combat
            // damage to that creature." With several attackers it divides its damage among them.
            CombatRole::Blocking { attackers, .. } => {
                if attackers.is_multiple() {
                    let (divided, _) = divide_among(
                        s,
                        attackers.iter().collect(),
                        amount,
                        false,
                        lethal_targets(slot, lethal),
                    );
                    for &(other, n) in divided.iter() {
                        push(Recipient::Permanent(s.object_ref(other)), n)?;
                    }
                } else if let Some(other) = attackers.first() {
                    push(Recipient::Permanent(s.object_ref(other)), amount)?;
                }
            }
            CombatRole::Attacking {
                blocked: true,
                target,
                ..
            } => {
                let blockers = blockers_of(s, slot);
                let trample = has(s, slot, Keywords::TRAMPLE);
                let excess = if let Some(blocker) = blockers.sole() {
                    let n = if trample {
                        lethal_damage(s, blocker).min(amount)
                    } else {
                        amount
                    };
                    push(Recipient::Permanent(s.object_ref(blocker)), n)?;
                    amount - n
                } else {
                    let (divided, excess) =
                        divide_among(s, blockers, amount, trample, lethal_targets(slot, lethal));
                    for &(other, n) in divided.iter() {
                        push(Recipient::Permanent(s.object_ref(other)), n)?;
                    }
                    excess
                };
                if let Some(to) = attacked(s, *target)
                    && excess > 0
                {
                    push(to, excess)?;
                }
            }
            // CR 510.1b: "An unblocked creature assigns its combat damage to the player,
            // planeswalker, or battle it's attacking. If it isn't currently attacking anything
            // (if, for example, it was attacking a planeswalker that has left the
            // battlefield), it assigns no combat damage."
            CombatRole::Attacking { target, .. } => {
                if let Some(to) = attacked(s, *target) {
                    push(to, amount)?;
                }
            }
            CombatRole::None => {}
        }
    }
    Ok(assigned)
}

/// Assigns lethal damage to the chosen creatures and distributes any remainder in battlefield
/// order. Slots reflect the deal and shuffles, so ordering by slot would make the outcome depend
/// on the seed. Each recipient gets at most one entry, so the queue fits every recipient.
///
/// An attacker with trample ([CR 702.19b]) assigns what is left once every blocker is assigned
/// lethal damage to what it attacks, a lone blocker taking lethal damage only: "Once all those
/// blocking creatures are assigned lethal damage, any excess damage is assigned as its
/// controller chooses among those blocking creatures and the player, planeswalker, or battle the
/// creature is attacking." Of those choices only the most to what it attacks is offered:
/// damage over lethal on a blocker changes nothing that the cards of the pool read. With no
/// blocker left, it all goes to what it attacks ([CR 702.19d]): "If an attacking creature with
/// trample or trample over planeswalkers is blocked, but there are no creatures blocking it when
/// damage is assigned, its damage is assigned to the defending player and/or planeswalker as
/// though all blocking creatures have been assigned lethal damage."
///
/// [CR 702.19b]: https://yawgatog.com/resources/magic-rules/#R70219b
/// [CR 702.19d]: https://yawgatog.com/resources/magic-rules/#R70219d
fn divide_among<L: Layout>(
    s: &State<L>,
    recipients: Bitboard<L::Slot>,
    mut left: u8,
    trample: bool,
    killed: Bitboard<L::Slot>,
) -> (L::Queue<(L::Slot, u8), BATTLEFIELD>, u8) {
    let mut out: L::Queue<(L::Slot, u8), BATTLEFIELD> = Default::default();
    let entered = |set: Bitboard<L::Slot>| {
        s.battlefield()
            .iter()
            .copied()
            .filter(move |&b| set.contains(b))
    };
    for b in entered(killed) {
        let n = lethal_damage(s, b).min(left);
        left -= n;
        out.push_bounded((b, n));
    }
    if trample && killed == recipients {
        return (out, left);
    }
    if let Some(first) = out.as_mut_slice().first_mut() {
        first.1 += left;
        return (out, 0);
    }
    for b in entered(recipients & !killed) {
        let n = (lethal_damage(s, b) - 1).min(left);
        left -= n;
        if n > 0 {
            out.push_bounded((b, n));
        }
    }
    (out, 0)
}
