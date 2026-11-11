//! Combat: declaring attackers and blockers one pick at a time, and combat damage
//! (architecture §11, decision D8, CR 506 to 511).
//!
//! Declarations and damage divisions are explicit machines whose step lives in
//! [`Pending`]; every candidate offered is legal, so a declaration never has to be undone
//! (CR 508.1, 509.1 return the game to before an illegal one).

use crate::bitboard::Bitboard;
use crate::card::{Keywords, Types};
use crate::decision::{Ask, Candidates, Choice, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Event, Recipient, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::state::{ObjectFlags, ObjectRef, Pending, State, TurnStep};
use crate::table::GameTable;

/// The defending player ([CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062)):
/// "During the combat phase of a two-player game, the nonactive player is the defending
/// player".
fn defender<L: Layout>(s: &State<L>) -> Seat {
    s.turn.active.opponent()
}

fn flags<L: Layout>(s: &State<L>, slot: L::Slot) -> ObjectFlags {
    s.object(slot).flags
}

fn is_creature<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    s.derived(slot).types.contains(Types::CREATURE)
}

fn has<L: Layout>(s: &State<L>, slot: L::Slot, keyword: Keywords) -> bool {
    s.derived(slot).keywords.contains(keyword)
}

/// The battlefield objects with a flag.
fn with_flag<L: Layout>(s: &State<L>, flag: u32) -> Bitboard<L::Slot> {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| flags(s, slot).has(flag))
        .collect()
}

/// The creatures blocking an attacker.
fn blockers_of<L: Layout>(s: &State<L>, attacker: L::Slot) -> Bitboard<L::Slot> {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| {
            let o = s.object(slot);
            o.flags.has(ObjectFlags::BLOCKING) && o.combat == Some(attacker)
        })
        .collect()
}

/// The objects another object's `combat` names: the attacked planeswalkers and the blocked
/// attackers with a blocker still blocking them. Two Ogres, one blocked by a 2/2 and one by
/// a 1/3, share a signature but are not interchangeable to a third blocker.
pub(crate) fn related<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    s.battlefield()
        .iter()
        .filter_map(|&slot| s.object(slot).combat)
        .collect()
}

/// Whether any creature can attack now, so that the declaration is worth starting.
///
/// [CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a): "The active player
/// chooses which creatures that they control, if any, will attack. The chosen creatures must
/// be untapped, they can't also be battles, and each one must either have haste or have been
/// controlled by the active player continuously since the turn began." None of the cards has
/// haste.
fn may_attack<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    let o = s.object(slot);
    s.controller(slot) == s.turn.active
        && is_creature(s, slot)
        && !o.flags.has(ObjectFlags::TAPPED)
        && !o.flags.has(ObjectFlags::SUMMONING_SICK)
        && !o.flags.has(ObjectFlags::ATTACKING)
}

/// Whether the active player has a creature that can attack now, so that declaring attackers
/// is a decision.
pub(crate) fn can_declare_attackers<L: Layout>(s: &State<L>) -> bool {
    s.battlefield().iter().any(|&slot| may_attack(s, slot))
}

/// The declare attackers step's turn-based action begins
/// ([CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)): "First, the active player
/// declares attackers. This turn-based action doesn't use the stack."
pub(crate) fn begin_attackers<L: Layout>(s: &mut State<L>) {
    if can_declare_attackers(s) {
        s.pending = Pending::DeclareAttackers { attacker: None };
    }
}

/// The next pick of the attack declaration: a creature or done, or, for the creature just
/// chosen, what it attacks.
///
/// [CR 508.1b](https://yawgatog.com/resources/magic-rules/#R5081b): the active player
/// "announces which player, planeswalker, or battle each of the chosen creatures is
/// attacking." Offered only when the defending player controls a planeswalker; otherwise the
/// one candidate is applied silently (§7.2).
pub(crate) fn attack_decision<L: Layout>(
    s: &State<L>,
    attacker: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    let (player, kind) = if attacker.is_none() {
        list.push(Opt::Done)?;
        let able: Bitboard<L::Slot> = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&slot| may_attack(s, slot))
            .collect();
        for slot in collapse(s, able).iter() {
            list.push(Opt::Attack(slot))?;
        }
        (s.turn.active, DecisionKind::Attacker)
    } else {
        list = attack_targets(s, defender(s));
        (s.turn.active, DecisionKind::AttackTarget)
    };
    Ok(Decision {
        player,
        kind,
        candidates: Candidates::Options(list),
        ask: attacker.map_or(Ask::Plain, |slot| Ask::AttackTarget {
            attackers: Bitboard::single(slot),
        }),
    })
}

/// What a creature can attack: the defending player, then each planeswalker they control.
pub(crate) fn attack_targets<L: Layout>(s: &State<L>, defending: Seat) -> L::Queue<Opt<L>, 64> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    // No player controls 63 planeswalkers.
    let _ = list.push(Opt::Defender(None));
    for &slot in s.battlefield() {
        if s.controller(slot) == defending && s.derived(slot).types.contains(Types::PLANESWALKER) {
            let _ = list.push(Opt::Defender(Some(slot)));
        }
    }
    list
}

/// Applies one pick of the attack declaration.
pub(crate) fn attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    opt: Opt<L>,
) -> Result<(), Full> {
    match opt {
        Opt::Attack(slot) => {
            // CR 508.1k: "Each chosen creature still controlled by the active player becomes
            // an attacking creature."
            s.declare_attacker(slot);
            s.pending = Pending::DeclareAttackers {
                attacker: Some(slot),
            };
            Ok(())
        }
        Opt::Defender(target) => {
            let Pending::DeclareAttackers {
                attacker: Some(attacker),
            } = s.pending
            else {
                unreachable!("an attack target without an attacker");
            };
            s.choose_attacked(attacker, target);
            s.pending = Pending::DeclareAttackers { attacker: None };
            Ok(())
        }
        _ => finish_attack(s, t, sink),
    }
}

/// Finishes the attack declaration: the attackers tap, then attack triggers trigger.
///
/// [CR 508.1f](https://yawgatog.com/resources/magic-rules/#R5081f): "The active player taps
/// the chosen creatures. Tapping a creature when it's declared as an attacker isn't a cost;
/// attacking simply causes creatures to become tapped."
///
/// [CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a): "An ability that reads
/// "Whenever [a creature] attacks, . . ." triggers if that creature is declared as an
/// attacker. ... Such abilities won't trigger if a creature is put onto the battlefield
/// attacking." The event names only the declared attackers
/// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): creatures put onto the
/// battlefield attacking, "for the purposes of trigger events and effects, ... never
/// 'attacked.'"
fn finish_attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    s.pending = Pending::None;
    let attackers = with_flag(s, ObjectFlags::ATTACKING);
    // In battlefield order, never slot order: the taps' events are seen in this order, and a
    // slot number says nothing about a card (architecture §13). Tapping moves nothing. One
    // attacker has no order, and needs no walk of the battlefield.
    let tap = |s: &mut State<L>, sink: &mut Sink<L>, slot| {
        if has(s, slot, Keywords::VIGILANCE) {
            return Ok(());
        }
        event::set_tapped(s, t, sink, s.object_ref(slot), true)
    };
    let mut untapped = attackers;
    if attackers.count() > 1 {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if untapped.contains(slot) {
                untapped.remove(slot);
                tap(s, sink, slot)?;
                if untapped.is_empty() {
                    break;
                }
            }
        }
    }
    for slot in untapped.iter() {
        tap(s, sink, slot)?;
    }
    if attackers.is_empty() {
        return Ok(());
    }
    s.turn.attacked = true;
    let player = s.turn.active;
    event::settle(s, t, sink, Event::Attack { player, attackers })
}

/// Whether `blocker` may block `attacker`: an attacker attacking the defending player or a
/// planeswalker they control (one attacking nothing can't be blocked), within its evasion.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the chosen
/// creatures, the defending player chooses one creature for it to block that's attacking that
/// player, a planeswalker they control, or a battle they protect."
///
/// [CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b): "A creature with flying
/// can't be blocked except by creatures with flying and/or reach."
fn can_block<L: Layout>(s: &State<L>, blocker: L::Slot, attacker: L::Slot) -> bool {
    let a = s.object(attacker);
    a.flags.has(ObjectFlags::ATTACKING)
        && !(a.flags.has(ObjectFlags::ATTACKS_PLANESWALKER) && a.combat.is_none())
        && (!has(s, attacker, Keywords::FLYING)
            || has(s, blocker, Keywords::FLYING)
            || has(s, blocker, Keywords::REACH))
}

/// Whether a creature may still be declared as a blocker: an untapped creature of the
/// defending player that blocks nothing yet and can block some attacker.
///
/// [CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "The defending player
/// chooses which creatures they control, if any, will block. The chosen creatures must be
/// untapped and they can't also be battles."
fn may_block<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    let o = s.object(slot);
    s.controller(slot) == defender(s)
        && is_creature(s, slot)
        && !o.flags.has(ObjectFlags::TAPPED)
        && !o.flags.has(ObjectFlags::BLOCKING)
        && s.battlefield().iter().any(|&a| can_block(s, slot, a))
}

/// Whether the defending player has a creature that can block now, so that declaring blockers
/// is a decision.
pub(crate) fn can_declare_blockers<L: Layout>(s: &State<L>) -> bool {
    s.battlefield().iter().any(|&slot| may_block(s, slot))
}

/// The declare blockers step's turn-based action begins
/// ([CR 509.1](https://yawgatog.com/resources/magic-rules/#R5091)): "First, the defending
/// player declares blockers. This turn-based action doesn't use the stack."
pub(crate) fn begin_blockers<L: Layout>(s: &mut State<L>) {
    if can_declare_blockers(s) {
        s.pending = Pending::DeclareBlockers { blocker: None };
    }
}

/// The next pick of the block declaration: a creature or done, or, for the creature just
/// chosen, the attacker it blocks. Evasion prunes both.
pub(crate) fn block_decision<L: Layout>(
    s: &State<L>,
    blocker: Option<L::Slot>,
) -> Result<Decision<L>, Full> {
    let player = defender(s);
    if let Some(blocker) = blocker {
        let attackers = s
            .battlefield()
            .iter()
            .copied()
            .filter(|&a| can_block(s, blocker, a))
            .collect();
        return Ok(Decision {
            player,
            kind: DecisionKind::BlockTarget,
            candidates: Candidates::Objects(collapse(s, attackers)),
            ask: Ask::Plain,
        });
    }
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    list.push(Opt::Done)?;
    let able = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| may_block(s, slot))
        .collect();
    for slot in collapse(s, able).iter() {
        list.push(Opt::Block(slot))?;
    }
    Ok(Decision {
        player,
        kind: DecisionKind::Blocker,
        candidates: Candidates::Options(list),
        ask: Ask::Plain,
    })
}

/// Declares a blocker, whose attacker is the next pick.
///
/// [CR 509.1g](https://yawgatog.com/resources/magic-rules/#R5091g): "Each chosen creature
/// still controlled by the defending player becomes a blocking creature."
pub(crate) fn block_with<L: Layout>(s: &mut State<L>, blocker: L::Slot) {
    s.declare_blocker(blocker);
    s.pending = Pending::DeclareBlockers {
        blocker: Some(blocker),
    };
}

/// Has the blocker just declared block `attacker`.
///
/// [CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h): "An attacking creature
/// with one or more creatures declared as blockers for it becomes a blocked creature". It
/// becomes blocked as soon as its first blocker is chosen, so a blocked and an unblocked
/// attacker are never offered as interchangeable to the next blocker.
pub(crate) fn block_attacker<L: Layout>(s: &mut State<L>, attacker: L::Slot) {
    let Pending::DeclareBlockers {
        blocker: Some(blocker),
    } = s.pending
    else {
        unreachable!("a block target without a blocker");
    };
    s.block(blocker, attacker);
    s.pending = Pending::DeclareBlockers { blocker: None };
}

/// Finishes the block declaration.
pub(crate) fn finish_blocks<L: Layout>(s: &mut State<L>) {
    s.pending = Pending::None;
}

/// Whether the combat damage step gets a first-strike step before it
/// ([CR 510.4](https://yawgatog.com/resources/magic-rules/#R5104)): "If at least one
/// attacking or blocking creature has first strike ... as the combat damage step begins, the
/// only creatures that assign combat damage in that step are those with first strike ...
/// After that step, instead of proceeding to the end of combat step, the phase gets a second
/// combat damage step." None of the cards has double strike.
pub(crate) fn first_strike_step<L: Layout>(s: &State<L>) -> bool {
    s.battlefield().iter().any(|&slot| {
        flags(s, slot).has(ObjectFlags::ATTACKING | ObjectFlags::BLOCKING)
            && has(s, slot, Keywords::FIRST_STRIKE)
    })
}

/// Whether a creature in combat assigns combat damage in this step: in a first-strike step
/// those with first strike; in the other, "the remaining attackers and blockers that had
/// neither first strike nor double strike as the first combat damage step began" (CR 510.4).
fn assigns_now<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    if !flags(s, slot).has(ObjectFlags::ATTACKING | ObjectFlags::BLOCKING) {
        return false;
    }
    if s.turn.step == TurnStep::FirstStrikeDamage {
        has(s, slot, Keywords::FIRST_STRIKE)
    } else {
        !flags(s, slot).has(ObjectFlags::STRUCK_FIRST)
    }
}

/// The combat damage a creature assigns
/// ([CR 510.1a](https://yawgatog.com/resources/magic-rules/#R5101a)): "Each attacking creature
/// and each blocking creature assigns combat damage equal to its power. Creatures that would
/// assign 0 or less damage this way don't assign combat damage at all."
fn power<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    u8::try_from(s.derived(slot).power.max(0)).unwrap_or(0)
}

/// The damage that would be lethal to a creature: its toughness less the damage already
/// marked on it ([CR 120.6](https://yawgatog.com/resources/magic-rules/#R1206)): "If the total
/// damage marked on a creature is greater than or equal to its toughness, that creature has
/// been dealt lethal damage". At least 1.
fn lethal<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    let left = i16::from(s.derived(slot).toughness) - i16::from(s.object(slot).damage);
    u8::try_from(left.clamp(1, 255)).unwrap_or(1)
}

/// The next attacker whose damage its controller divides: blocked by two or more creatures
/// and assigning damage now, and not yet divided.
fn next_division<L: Layout>(s: &State<L>, done: Bitboard<L::Slot>) -> Option<L::Slot> {
    s.battlefield().iter().copied().find(|&slot| {
        flags(s, slot).has(ObjectFlags::ATTACKING)
            && flags(s, slot).has(ObjectFlags::BLOCKED)
            && !done.contains(slot)
            && assigns_now(s, slot)
            && power(s, slot) > 0
            && blockers_of(s, slot).count() >= 2
    })
}

/// The combat damage step's turn-based actions begin
/// ([CR 510.1](https://yawgatog.com/resources/magic-rules/#R5101)): "First, the active player
/// announces how each attacking creature assigns its combat damage, then the defending player
/// announces how each blocking creature assigns its combat damage." A blocking creature
/// blocks one attacker, so only the attackers' divisions are choices; when none is, the
/// damage is dealt at once.
pub(crate) fn begin_damage<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.turn.step == TurnStep::FirstStrikeDamage {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if assigns_now(s, slot) {
                s.mark_struck_first(slot);
            }
        }
    }
    if next_division(s, Bitboard::EMPTY).is_some() {
        s.pending = Pending::CombatDamage {
            done: Bitboard::EMPTY,
            lethal: Bitboard::EMPTY,
        };
        Ok(())
    } else {
        deal(s, t, sink, Bitboard::EMPTY)
    }
}

/// The next pick of a damage division: a blocker to deal lethal damage to, or done.
///
/// [CR 510.1c](https://yawgatog.com/resources/magic-rules/#R5101c): "If two or more creatures
/// are blocking it, it assigns its combat damage to those creatures divided as its controller
/// chooses among them." Divisions are offered by their distinct outcomes (§11): which
/// blockers are dealt lethal damage, picked one at a time while the damage left covers
/// another. "Done" is offered once the damage left can go somewhere without killing another
/// blocker: onto a blocker already dealt lethal damage, or below lethal on the others. None
/// of the cards deals noncombat damage to creatures, so damage on a survivor never matters.
pub(crate) fn division_decision<L: Layout>(
    s: &State<L>,
    done: Bitboard<L::Slot>,
    chosen: Bitboard<L::Slot>,
) -> Result<Decision<L>, Full> {
    let attacker = next_division(s, done).expect("a division without an attacker");
    let blockers = blockers_of(s, attacker);
    let killed = blockers & chosen;
    let spent: u32 = killed.iter().map(|b| u32::from(lethal(s, b))).sum();
    let left = u32::from(power(s, attacker)) - spent;
    let absorbable: u32 = (blockers & !chosen)
        .iter()
        .map(|b| u32::from(lethal(s, b)) - 1)
        .sum();
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    if !killed.is_empty() || left <= absorbable {
        list.push(Opt::Done)?;
    }
    let more = (blockers & !chosen)
        .iter()
        .filter(|&b| u32::from(lethal(s, b)) <= left)
        .collect();
    for b in collapse(s, more).iter() {
        list.push(Opt::Lethal(b))?;
    }
    Ok(Decision {
        player: s.turn.active,
        kind: DecisionKind::DamageDivision,
        candidates: Candidates::Options(list),
        ask: Ask::Plain,
    })
}

/// The smart-stop recommendation for the next pick under [CR 510.1c](https://yawgatog.com/resources/magic-rules/#R5101c)
/// ("divided as its controller chooses among them"). Prefer killing more blockers by
/// taking the cheapest lethal assignment first, breaking ties by power. The pick
/// can be silent if it is the only distinct lethal pick or all killable blockers fit.
pub fn preferred_damage<L: Layout>(s: &State<L>, d: &Decision<L>) -> Option<(Choice, bool)> {
    let Pending::CombatDamage {
        done,
        lethal: chosen,
    } = s.pending
    else {
        return None;
    };
    let Candidates::Options(options) = &d.candidates else {
        return None;
    };
    let attacker = next_division(s, done)?;
    let blockers = blockers_of(s, attacker);
    let spent: u32 = (blockers & chosen)
        .iter()
        .map(|b| u32::from(lethal(s, b)))
        .sum();
    let left = u32::from(power(s, attacker)) - spent;
    let cost: u32 = (blockers & !chosen)
        .iter()
        .map(|b| u32::from(lethal(s, b)))
        .filter(|&cost| cost <= left)
        .sum();
    let picks = options.iter().enumerate().filter_map(|(i, option)| {
        let Opt::Lethal(slot) = *option else {
            return None;
        };
        Some((Choice(i as u16), lethal(s, slot), s.derived(slot).power))
    });
    let unambiguous = cost <= left || picks.clone().count() == 1;
    picks
        .min_by_key(|&(_, cost, power)| (cost, std::cmp::Reverse(power)))
        .map(|(pick, _, _)| (pick, unambiguous))
}

/// Applies one pick of a damage division, dealing the damage once every division is made.
pub(crate) fn divide<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    done: Bitboard<L::Slot>,
    mut chosen: Bitboard<L::Slot>,
    opt: Opt<L>,
) -> Result<(), Full> {
    let mut done = done;
    if let Opt::Lethal(b) = opt {
        chosen.insert(b);
    } else {
        let attacker = next_division(s, done).expect("a division without an attacker");
        done.insert(attacker);
        if next_division(s, done).is_none() {
            s.pending = Pending::None;
            return deal(s, t, sink, chosen);
        }
    }
    s.pending = Pending::CombatDamage {
        done,
        lethal: chosen,
    };
    Ok(())
}

/// One assignment of combat damage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Assignment<L: Layout = Small> {
    source: ObjectRef<L>,
    to: Option<Recipient<L>>,
    amount: u8,
}

/// Deals the combat damage of this step, `lethal` holding the blockers the divisions chose
/// to destroy.
///
/// [CR 510.2](https://yawgatog.com/resources/magic-rules/#R5102): "Second, all combat damage
/// that's been assigned is dealt simultaneously." Every assignment is made from the creatures
/// as they are before any damage is dealt.
fn deal<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    lethal: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let mut assigned: L::Queue<Assignment<L>, 128> = Default::default();
    for &slot in s.battlefield() {
        if !assigns_now(s, slot) || power(s, slot) == 0 {
            continue;
        }
        let o = s.object(slot);
        let source = s.object_ref(slot);
        let amount = power(s, slot);
        let mut push = |to: Recipient<L>, amount: u8| {
            assigned.push(Assignment {
                source,
                to: Some(to),
                amount,
            })
        };
        if o.flags.has(ObjectFlags::BLOCKING) {
            // CR 510.1d: "A blocking creature assigns combat damage to the creatures it's
            // blocking. If it isn't currently blocking any creatures ..., it assigns no
            // combat damage."
            if let Some(other) = o.combat {
                push(Recipient::Permanent(s.object_ref(other)), amount)?;
            }
        } else if o.flags.has(ObjectFlags::BLOCKED) {
            for (b, n) in division(s, slot, lethal).iter() {
                push(Recipient::Permanent(s.object_ref(*b)), *n)?;
            }
        } else if o.flags.has(ObjectFlags::ATTACKS_PLANESWALKER) {
            // CR 510.1b: "An unblocked creature assigns its combat damage to the player,
            // planeswalker, or battle it's attacking. If it isn't currently attacking anything
            // (if, for example, it was attacking a planeswalker that has left the
            // battlefield), it assigns no combat damage."
            if let Some(other) = o.combat {
                push(Recipient::Permanent(s.object_ref(other)), amount)?;
            }
        } else {
            push(Recipient::Player(defender(s)), amount)?;
        }
    }
    for a in assigned.iter() {
        if let Some(to) = a.to {
            event::deal_damage(s, t, sink, a.source, to, a.amount, true)?;
        }
    }
    Ok(())
}

/// How a blocked attacker's damage divides among its blockers, given the blockers chosen to
/// be dealt lethal damage: each chosen one gets lethal damage, and what is left goes to the
/// first of them, or, if none was chosen, below lethal onto the others, in the order they
/// entered the battlefield. Slots are no order at all: the deal and shuffles put cards in
/// them at random, so a slot order would make the outcome depend on the seed. A lone blocker
/// gets everything (CR 510.1c: "If exactly one creature is blocking it, it assigns all its
/// combat damage to that creature").
fn division<L: Layout>(
    s: &State<L>,
    attacker: L::Slot,
    lethal_to: Bitboard<L::Slot>,
) -> L::Queue<(L::Slot, u8), 128> {
    let mut out: L::Queue<(L::Slot, u8), 128> = Default::default();
    let blockers = blockers_of(s, attacker);
    let mut left = power(s, attacker);
    if let (1, Some(blocker)) = (blockers.count(), blockers.first()) {
        let _ = out.push((blocker, left));
        return out;
    }
    let killed = blockers & lethal_to;
    let entered = |set: Bitboard<L::Slot>| {
        s.battlefield()
            .iter()
            .copied()
            .filter(move |&b| set.contains(b))
    };
    for b in entered(killed) {
        let n = lethal(s, b).min(left);
        left -= n;
        let _ = out.push((b, n));
    }
    if let Some(first) = out.as_mut_slice().first_mut() {
        first.1 += left;
        return out;
    }
    for b in entered(blockers & !killed) {
        let n = (lethal(s, b) - 1).min(left);
        left -= n;
        if n > 0 {
            let _ = out.push((b, n));
        }
    }
    out
}

/// Removes from combat what CR 506.4 removes while combat goes on: "A permanent is removed
/// from combat if it leaves the battlefield, if its controller ... changes, ... if it's a
/// planeswalker that's being attacked and stops being a planeswalker, ... or if it's an
/// attacking or blocking creature that ... stops being a creature". Leaving the battlefield
/// is handled as objects change zones; this handles the rest, after the layers settle.
///
/// [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A creature that's removed
/// from combat stops being an attacking, blocking, blocked, and/or unblocked creature. A
/// planeswalker or battle that's removed from combat stops being attacked."
pub(crate) fn prune<L: Layout>(s: &mut State<L>) {
    let active = s.turn.active;
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        let o = *s.object(slot);
        let side = if o.flags.has(ObjectFlags::ATTACKING) {
            active
        } else if o.flags.has(ObjectFlags::BLOCKING) {
            active.opponent()
        } else {
            continue;
        };
        if !is_creature(s, slot) || s.controller(slot) != side {
            s.remove_from_combat(slot);
        } else if o.flags.has(ObjectFlags::ATTACKS_PLANESWALKER)
            && let Some(walker) = o.combat
            && (s.controller(walker) != active.opponent()
                || !s.derived(walker).types.contains(Types::PLANESWALKER))
        {
            s.forget_attacked(slot);
        }
    }
}

/// Whether the game is in the combat phase, after attackers may have been declared.
pub(crate) fn in_combat(step: TurnStep) -> bool {
    matches!(
        step,
        TurnStep::DeclareAttackers
            | TurnStep::DeclareBlockers
            | TurnStep::FirstStrikeDamage
            | TurnStep::CombatDamage
            | TurnStep::EndOfCombat
    )
}

/// Ends combat ([CR 511.3](https://yawgatog.com/resources/magic-rules/#R5113)): "As soon as
/// the end of combat step ends, all creatures, battles, and planeswalkers are removed from
/// combat."
pub(crate) fn end_combat<L: Layout>(s: &mut State<L>) {
    s.turn.attacked = false;
    s.end_combat();
}
