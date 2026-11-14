//! Combat damage ([CR 510]): which creatures assign it in a step, how an attacker blocked by
//! more than one creature or with trample divides it, and dealing it.
//!
//! [CR 510]: https://yawgatog.com/resources/magic-rules/#R510

use super::{blockers_of, has, in_combat_now};
use crate::bitboard::Bitboard;
use crate::card::{Keywords, Prompt, Purpose};
use crate::damage;
use crate::decision::{Candidates, Choice, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{Recipient, Sink};
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::{Layout, Queue, Small};
use crate::state::{AttackTarget, BATTLEFIELD, CombatRole, ObjectRef, Pending, State, TurnStep};
use crate::table::GameTable;

/// Whether the combat damage step gets a first-strike step before it ([CR 510.4]): "If at least one
/// attacking or blocking creature has first strike ... as the combat damage step begins, the only
/// creatures that assign combat damage in that step are those with first strike ... After that
/// step, instead of proceeding to the end of combat step, the phase gets a second combat damage
/// step." None of the cards has double strike.
///
/// [CR 510.4]: https://yawgatog.com/resources/magic-rules/#R5104
pub(crate) fn first_strike_step<L: Layout>(s: &State<L>) -> bool {
    s.battlefield()
        .iter()
        .any(|&slot| in_combat_now(s, slot) && has(s, slot, Keywords::FIRST_STRIKE))
}

/// Whether a creature in combat assigns combat damage in this step: in a first-strike step
/// those with first strike; in the other, "the remaining attackers and blockers that had
/// neither first strike nor double strike as the first combat damage step began" (CR 510.4).
fn assigns_now<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    if !in_combat_now(s, slot) {
        return false;
    }
    if s.turn().step() == TurnStep::FirstStrikeDamage {
        has(s, slot, Keywords::FIRST_STRIKE)
    } else {
        !s.object(slot).struck_first()
    }
}

/// The combat damage a creature assigns ([CR 510.1a]): "Each attacking creature and each blocking
/// creature assigns combat damage equal to its power. Creatures that would assign 0 or less damage
/// this way don't assign combat damage at all."
///
/// [CR 510.1a]: https://yawgatog.com/resources/magic-rules/#R5101a
fn power<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    u8::try_from(s.derived(slot).power.max(0)).unwrap_or(0)
}

/// The damage that would be lethal to a creature: its toughness less the damage already marked on
/// it ([CR 120.6]): "If the total damage marked on a creature is greater than or equal to its
/// toughness, that creature has been dealt lethal damage". At least 1.
///
/// [CR 120.6]: https://yawgatog.com/resources/magic-rules/#R1206
fn lethal<L: Layout>(s: &State<L>, slot: L::Slot) -> u8 {
    let left = i16::from(s.derived(slot).toughness) - i16::from(s.object(slot).damage());
    u8::try_from(left.clamp(1, 255)).unwrap_or(1)
}

/// What an attacker attacks, as a recipient of its damage: nothing once a planeswalker it
/// attacked has left the battlefield (CR 510.1b).
fn attacked<L: Layout>(s: &State<L>, target: AttackTarget<L>) -> Option<Recipient<L>> {
    match target {
        AttackTarget::Player(player) => Some(Recipient::Player(player)),
        AttackTarget::Planeswalker(walker) => Some(Recipient::Permanent(s.object_ref(walker))),
        AttackTarget::Removed => None,
    }
}

/// The next attacker whose damage its controller divides: blocked by two or more creatures
/// and assigning damage now, and not yet divided. One with trample blocked by one creature
/// divides nothing: lethal damage to the blocker and the rest to what it attacks is the one
/// division worth offering (see [`division`]).
fn next_division<L: Layout>(s: &State<L>, done: Bitboard<L::Slot>) -> Option<L::Slot> {
    s.battlefield().iter().copied().find(|&slot| {
        s.object(slot).is_blocked()
            && !done.contains(slot)
            && assigns_now(s, slot)
            && power(s, slot) > 0
            && blockers_of(s, slot).count() >= 2
    })
}

/// The combat damage step's turn-based actions begin ([CR 510.1]): "First, the active player
/// announces how each attacking creature assigns its combat damage, then the defending player
/// announces how each blocking creature assigns its combat damage." A blocking creature blocks one
/// attacker, so only the attackers' divisions are choices; when none is, the damage is dealt at
/// once.
///
/// [CR 510.1]: https://yawgatog.com/resources/magic-rules/#R5101
pub(crate) fn begin_damage<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.turn().step() == TurnStep::FirstStrikeDamage {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if assigns_now(s, slot) {
                s.mark_struck_first(slot);
            }
        }
    }
    if next_division(s, Bitboard::EMPTY).is_some() {
        s.ask(Pending::CombatDamage {
            done: Bitboard::EMPTY,
            lethal: Bitboard::EMPTY,
        });
        Ok(())
    } else {
        deal(s, t, sink, Bitboard::EMPTY)
    }
}

/// The next pick of a damage division: a blocker to deal lethal damage to, or done.
///
/// [CR 510.1c](https://yawgatog.com/resources/magic-rules/#R5101c): "If two or more creatures are
/// blocking it, it assigns its combat damage to those creatures divided as its controller chooses
/// among them." Divisions are offered by their distinct outcomes (§11): which blockers are dealt
/// lethal damage, picked one at a time while the damage left covers another. "Done" is offered once
/// the damage left can go somewhere without killing another blocker: onto a blocker already dealt
/// lethal damage, or below lethal on the others. None of the cards deals noncombat damage to
/// creatures, so damage on a survivor never matters. For an attacker with trample, "done" with
/// every blocker picked assigns what is left to what it attacks ([`division`]).
pub(crate) fn division_decision<L: Layout>(
    s: &State<L>,
    done: Bitboard<L::Slot>,
    chosen: Bitboard<L::Slot>,
) -> Result<Decision<L>, Full> {
    let attacker = next_division(s, done).expect("a division without an attacker");
    let blockers = blockers_of(s, attacker);
    let killed = blockers & chosen;
    let spent: u32 = killed.sum(|b| u32::from(lethal(s, b)));
    let left = u32::from(power(s, attacker)) - spent;
    let absorbable: u32 = (blockers & !chosen).sum(|b| u32::from(lethal(s, b)) - 1);
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    if !killed.is_empty() || left <= absorbable {
        list.push(Opt::Done)?;
    }
    let more = (blockers & !chosen).filter(|b| u32::from(lethal(s, b)) <= left);
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    for b in s.in_candidate_order(collapse(s, more)) {
        list.push(Opt::Lethal(b))?;
    }
    Ok(Decision {
        player: s.turn().active(),
        kind: DecisionKind::DamageDivision,
        candidates: Candidates::Options(list),
        prompt: Prompt::new(Purpose::CombatDamage),
        about: Bitboard::EMPTY,
    })
}

/// The smart-stop recommendation for the next pick under [CR 510.1c] ("divided as its controller
/// chooses among them"). Prefer killing more blockers by taking the cheapest lethal assignment
/// first, breaking ties by power. The pick can be silent if it is the only distinct lethal pick or
/// all killable blockers fit.
///
/// [CR 510.1c]: https://yawgatog.com/resources/magic-rules/#R5101c
pub fn preferred_damage<L: Layout>(s: &State<L>, d: &Decision<L>) -> Option<(Choice, bool)> {
    let Pending::CombatDamage {
        done,
        lethal: chosen,
    } = *s.pending()
    else {
        return None;
    };
    let Candidates::Options(options) = &d.candidates else {
        return None;
    };
    let attacker = next_division(s, done)?;
    let blockers = blockers_of(s, attacker);
    let spent: u32 = (blockers & chosen).sum(|b| u32::from(lethal(s, b)));
    let left = u32::from(power(s, attacker)) - spent;
    let cost: u32 = (blockers & !chosen).sum(|b| {
        let cost = u32::from(lethal(s, b));
        if cost <= left { cost } else { 0 }
    });
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
            s.settle_question();
            return deal(s, t, sink, chosen);
        }
    }
    s.ask(Pending::CombatDamage {
        done,
        lethal: chosen,
    });
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
/// [CR 510.2](https://yawgatog.com/resources/magic-rules/#R5102): "Second, all combat damage that's
/// been assigned is dealt simultaneously." Every assignment is made from the creatures as they are
/// before any damage is dealt, and the events of the damage are kept as one group. Damage to a
/// player is the life they lose (CR 120.3a), in the one event.
fn deal<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    lethal: Bitboard<L::Slot>,
) -> Result<(), Full> {
    sink.together();
    let dealt = deal_assigned(s, t, sink, lethal);
    sink.apart();
    dealt
}

/// [`deal`], inside the group it opens.
#[inline]
fn deal_assigned<L: Layout>(
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
        match *o.combat() {
            // CR 510.1d: "A blocking creature assigns combat damage to the creatures it's
            // blocking. If it isn't currently blocking any creatures ..., it assigns no
            // combat damage." A creature that blocks more than one attacker divides its
            // damage as an attacker does (CR 510.1d), which no card makes happen yet: it
            // assigns all of it to the first.
            CombatRole::Blocking { attackers, .. } => {
                if let Some(other) = attackers.first() {
                    push(Recipient::Permanent(s.object_ref(other)), amount)?;
                }
            }
            CombatRole::Attacking {
                blocked: true,
                target,
                ..
            } => {
                let (blockers, excess) = division(s, slot, lethal);
                for (b, n) in blockers.iter() {
                    push(Recipient::Permanent(s.object_ref(*b)), *n)?;
                }
                if let Some(to) = attacked(s, target)
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
                if let Some(to) = attacked(s, target) {
                    push(to, amount)?;
                }
            }
            CombatRole::None => {}
        }
    }
    for a in assigned.iter() {
        if let Some(to) = a.to {
            damage::deal(&mut Game::new(s, t, sink), a.source, to, a.amount, true)?;
        }
    }
    Ok(())
}

/// How a blocked attacker's damage divides among its blockers, given the blockers chosen to
/// be dealt lethal damage, and what is left over for what it attacks. Each chosen blocker gets
/// lethal damage, and what is left goes to the first of them, or, if none was chosen, below
/// lethal onto the others, in the order they entered the battlefield. Slots are no order at
/// all: the deal and shuffles put cards in them at random, so a slot order would make the
/// outcome depend on the seed. A lone blocker gets everything (CR 510.1c: "If exactly one
/// creature is blocking it, it assigns all its combat damage to that creature").
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
fn division<L: Layout>(
    s: &State<L>,
    attacker: L::Slot,
    lethal_to: Bitboard<L::Slot>,
) -> (L::Queue<(L::Slot, u8), BATTLEFIELD>, u8) {
    // An entry for a blocker at most, and each blocker is a permanent: the list is as long
    // as a fixed layout's battlefield, so every push below has room.
    let mut out: L::Queue<(L::Slot, u8), BATTLEFIELD> = Default::default();
    let blockers = blockers_of(s, attacker);
    let mut left = power(s, attacker);
    let trample = has(s, attacker, Keywords::TRAMPLE);
    if let Some(blocker) = blockers.sole() {
        let n = if trample {
            lethal(s, blocker).min(left)
        } else {
            left
        };
        out.push_bounded((blocker, n));
        return (out, left - n);
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
        out.push_bounded((b, n));
    }
    if trample && killed == blockers {
        return (out, left);
    }
    if let Some(first) = out.as_mut_slice().first_mut() {
        first.1 += left;
        return (out, 0);
    }
    for b in entered(blockers & !killed) {
        let n = (lethal(s, b) - 1).min(left);
        left -= n;
        if n > 0 {
            out.push_bounded((b, n));
        }
    }
    (out, 0)
}
