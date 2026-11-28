//! Manual payment helpers for priced and multi-term mana abilities.
//!
//! Implements: none. Supports `mana::manual::manual_options` and `manual_pay` in rich mana games.

use crate::bitboard::Bitboard;
use crate::card::{ManaColor, ManaCost, ObjectAbility};
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::GameTable;

use super::super::pool::{helps, pay_unit};
use super::super::remembered::Paying;
use super::super::sources::Sources;
use super::super::units::Units;
use super::super::{CLASSES, KINDS};
use super::{activate_extra_step, each_pool_payment, pays_from_state, pays_from_state_with};

/// Offers priced and multi-term mana abilities whose price is payable from the pool and that
/// leave `remaining` payable afterwards.
#[cold]
#[inline(never)]
pub(in crate::mana) fn manual_extra_options<L: Layout>(
    remaining: ManaCost,
    pool: [u8; KINDS],
    sources: &Sources<L>,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    for (ei, extra) in sources.extras.iter().enumerate() {
        let slot = extra.members.min(|s| s).expect("an extra has a member");
        let mut extra_used = [0u8; CLASSES];
        extra_used[ei] = 1;
        for act in &extra.activations {
            let mut ok = false;
            let mut overflow = false;
            each_pool_payment(pool, act.price, &mut |_, pool_after_price| {
                if act
                    .each_made_split(&mut |made, restricted_made| {
                        let mut after = pool_after_price;
                        for (a, &m) in after.iter_mut().zip(&made) {
                            let Some(sum) = a.checked_add(m) else {
                                overflow = true;
                                return false;
                            };
                            *a = sum;
                        }
                        let mut spare = Units::NONE;
                        for (k, &n) in restricted_made.iter().enumerate() {
                            if n > 0 {
                                spare.add(1 << k, u16::from(n));
                            }
                        }
                        if pays_from_state_with(
                            after,
                            sources,
                            &Table::filled(0),
                            0,
                            &extra_used,
                            None,
                            remaining,
                            spare,
                        ) {
                            ok = true;
                            return false;
                        }
                        true
                    })
                    .is_err()
                {
                    overflow = true;
                }
                !ok && !overflow
            });
            if overflow {
                return Err(Full);
            }
            if !ok {
                continue;
            }
            if let Some(ability) = act.ability {
                out.push(Opt::Activate { slot, ability })?;
            } else {
                let kind = act.terms()[0].kinds.trailing_zeros() as usize;
                out.push(Opt::Mana {
                    slot,
                    color: ManaColor::ALL[kind],
                })?;
            }
        }
    }
    Ok(())
}

/// Whether spending one unit of `color` toward `remaining` after tapping a source leaves the rest
/// of `remaining` payable.
#[cold]
#[inline(never)]
pub(in crate::mana) fn manual_can_spend_after<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    color: ManaColor,
    paying: Paying<L>,
) -> Result<bool, Full> {
    if !helps(remaining, color.index()) {
        return Ok(false);
    }
    let sources = Sources::of(s, t, player, Bitboard::EMPTY, paying)?;
    let mut rest = remaining;
    pay_unit(&mut rest, color.index());
    let mut pool = s.player(player).mana().amounts_u8();
    if pool[color.index()] == 0 {
        return Ok(true);
    }
    pool[color.index()] -= 1;
    Ok(sources.units(pool, &Table::filled(0)).pays(rest)
        || pays_from_state(pool, &sources, &Table::filled(0), 0, &[0; CLASSES], rest))
}

/// Performs an offered `Opt::Activate` of a priced or multi-term mana ability: pays its price
/// from the pool in a way that leaves `remaining` payable, taps its source and adds its mana.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(in crate::mana) fn manual_pay_extra<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
    slot: L::Slot,
    ability: ObjectAbility,
    paying: Paying<L>,
) -> Result<(ManaCost, Option<ManaColor>), Full> {
    let sources = Sources::of(s, t, player, Bitboard::EMPTY, paying)?;
    let pool = s.player(player).mana().amounts_u8();
    let (ei, act) = sources
        .extras
        .iter()
        .enumerate()
        .find_map(|(ei, e)| {
            e.members.contains(slot).then(|| {
                e.activations
                    .iter()
                    .find(|a| a.ability == Some(ability))
                    .map(|a| (ei, *a))
            })?
        })
        .expect("an offered extra mana ability");
    let mut extra_used = [0u8; CLASSES];
    extra_used[ei] = 1;
    let mut chosen = None;
    let mut overflow = false;
    each_pool_payment(pool, act.price, &mut |price_spend, pool_after_price| {
        if act
            .each_made_split(&mut |made, restricted_made| {
                let mut after = pool_after_price;
                for (a, &m) in after.iter_mut().zip(&made) {
                    let Some(sum) = a.checked_add(m) else {
                        overflow = true;
                        return false;
                    };
                    *a = sum;
                }
                let mut spare = Units::NONE;
                for (k, &n) in restricted_made.iter().enumerate() {
                    if n > 0 {
                        spare.add(1 << k, u16::from(n));
                    }
                }
                if pays_from_state_with(
                    after,
                    &sources,
                    &Table::filled(0),
                    0,
                    &extra_used,
                    None,
                    remaining,
                    spare,
                ) {
                    chosen = Some((price_spend, made, restricted_made));
                    return false;
                }
                true
            })
            .is_err()
        {
            overflow = true;
        }
        chosen.is_none() && !overflow
    });
    if overflow {
        return Err(Full);
    }
    let (price_spend, made, restricted_made) =
        chosen.expect("a pool payment that leaves the cost payable");
    sink.together();
    let activated = activate_extra_step(
        s,
        t,
        sink,
        player,
        slot,
        &act,
        price_spend,
        made,
        restricted_made,
    );
    sink.apart();
    activated?;
    Ok((remaining, None))
}
