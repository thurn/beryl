//! Exact combat damage assignments for combats with banding.

mod trample;

use super::{assigner, assigns_now, attacked, power, recipients};
use crate::bitboard::Bitboard;
use crate::card::{Keywords, Part, Prompt, Purpose};
use crate::combat::{damage, has};
use crate::decision::{Decision, DecisionKind, Opt};
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::{Layout, Queue};
use crate::replacement::event::Sink;
use crate::state::{CombatRole, DamageDivision, DamageShare, Pending, State};
use crate::table::GameTable;

/// Begins with an empty assignment. No damage is dealt while any chooser is still assigning.
///
/// Under [CR 510.1], "First, the active player announces how each attacking creature assigns its
/// combat damage, then the defending player announces how each blocking creature assigns its
/// combat damage."
///
/// [CR 510.1]: https://yawgatog.com/resources/magic-rules/#R5101
#[cold]
#[inline(never)]
pub(super) fn begin<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    advance(s, t, sink, Bitboard::EMPTY, Default::default())
}

fn next_source<L: Layout>(s: &State<L>, done: Bitboard<L::Slot>) -> Option<L::Slot> {
    s.battlefield()
        .iter()
        .copied()
        .filter(|&slot| !done.contains(slot) && assigns_now(s, slot) && power(s, slot) > 0)
        .min_by_key(|&slot| match s.object(slot).combat() {
            CombatRole::Attacking { .. } if !has(s, slot, Keywords::TRAMPLE) => 0,
            CombatRole::Attacking { .. } => 1,
            CombatRole::Blocking { .. } | CombatRole::None => 2,
        })
}

/// Completes forced assignments, stopping at the first creature with a choice. The
/// attacking creatures without trample assign before tramplers, so a trampler in
/// a band can count damage its companions have assigned to its blockers.
fn advance<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut done: Bitboard<L::Slot>,
    mut shares: L::Queue<DamageShare<L>, 32>,
) -> Result<(), Full> {
    while let Some(source) = next_source(s, done) {
        let recipients = recipients(s, source);
        let amount = power(s, source);
        let mut dividing = DamageDivision {
            source,
            done,
            recipients,
            remaining: amount,
            shares,
        };
        if recipients.is_empty() {
            // CR 702.19d: "If an attacking creature with trample ... is blocked, but there
            // are no creatures blocking it when damage is assigned, its damage is assigned
            // to the defending player and/or planeswalker as though all blocking creatures
            // have been assigned lethal damage."
            if let CombatRole::Attacking { blocked, .. } = s.object(source).combat()
                && (!blocked || has(s, source, Keywords::TRAMPLE))
            {
                give(&mut dividing, None, amount)?;
            }
        } else if let Some(last) = recipients.sole()
            && !trample::allows(s, &dividing, last, amount - 1)
        {
            give(&mut dividing, Some(last), amount)?;
        } else {
            s.ask(Pending::ExactCombatDamage(dividing));
            return Ok(());
        }
        shares = dividing.shares;
        done.insert(source);
    }
    s.settle_question();
    deal(s, t, sink, shares.as_slice())
}

/// Offers a share for the first remaining recipient. Zero is a legal share. When the
/// last creature can take less than the remainder, the rest goes to the attacked object
/// or player.
#[cold]
#[inline(never)]
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    dividing: &DamageDivision<L>,
) -> Result<Decision<L>, Full> {
    let first = first_recipient(s, dividing);
    let mut options: L::Queue<_, { crate::decision::OPTIONS }> = Default::default();
    for amount in 0..=dividing.remaining {
        if trample::allows(s, dividing, first, amount) {
            options.push(Opt::Number(amount))?;
        }
    }
    let player = assigner(s, t, dividing.source);
    Ok(Decision::options(
        player,
        DecisionKind::DamageDivision,
        options,
        Prompt::new(Purpose::CombatDamage).part(Part::Count),
    )
    .about(Bitboard::single(first)))
}

fn first_recipient<L: Layout>(s: &State<L>, dividing: &DamageDivision<L>) -> L::Slot {
    s.battlefield()
        .iter()
        .copied()
        .find(|&slot| dividing.recipients.contains(slot))
        .expect("a damage recipient")
}

fn give<L: Layout>(
    dividing: &mut DamageDivision<L>,
    target: Option<L::Slot>,
    amount: u8,
) -> Result<(), Full> {
    if amount > 0 {
        dividing.shares.push(DamageShare {
            source: dividing.source,
            target,
            amount,
        })?;
        dividing.remaining -= amount;
    }
    Ok(())
}

#[cold]
#[inline(never)]
pub(super) fn answer<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut dividing: DamageDivision<L>,
    opt: Opt<L>,
) -> Result<(), Full> {
    let Opt::Number(amount) = opt else {
        unreachable!("exact damage offers an amount");
    };
    let first = first_recipient(s, &dividing);
    give(&mut dividing, Some(first), amount)?;
    dividing.recipients.remove(first);
    if dividing.recipients.is_empty() {
        let remainder = dividing.remaining;
        give(&mut dividing, None, remainder)?;
    }
    if dividing.remaining > 0 {
        s.ask(Pending::ExactCombatDamage(dividing));
        return Ok(());
    }
    dividing.done.insert(dividing.source);
    advance(s, t, sink, dividing.done, dividing.shares)
}

/// Under [CR 510.2], "Second, all combat damage that's been assigned is dealt simultaneously."
///
/// [CR 510.2]: https://yawgatog.com/resources/magic-rules/#R5102
fn deal<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    shares: &[DamageShare<L>],
) -> Result<(), Full> {
    use crate::replacement::event::Recipient;

    sink.together();
    let result = (|| {
        for share in shares {
            let source = s.object_ref(share.source);
            let target = if let Some(slot) = share.target {
                Some(Recipient::Permanent(s.object_ref(slot)))
            } else {
                s.object(share.source)
                    .attack_target()
                    .and_then(|target| attacked(s, target))
            };
            if let Some(target) = target {
                damage::deal(
                    &mut Game::new(s, t, sink),
                    source,
                    target,
                    share.amount,
                    true,
                )?;
            }
        }
        Ok(())
    })();
    sink.apart();
    result
}
