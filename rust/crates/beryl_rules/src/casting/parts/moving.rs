//! Checks spell costs that can supply cards for later parts of the same payment.
//!
//! Implements: none. Checks the payment order of the enclosing casting module.

use crate::bitboard::Bitboard;
use crate::card::{Castings, CostAction, CostPart, Face, Measure};
use crate::fixed::Full;
use crate::game::life;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue};
use crate::replacement::event::Sink;
use crate::state::{PayingObjects, StackEntry, StackItem, State, Zone};
use crate::table::GameTable;

use super::{candidates, graveyard_start, itself, pay_with_all, remember_cost_pick};

const INLINE_COST_PARTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CostPartRef(&'static CostPart);

impl Default for CostPartRef {
    fn default() -> Self {
        // The compact queue exposes only entries that have been pushed.
        Self(&CostPart::Life(0))
    }
}

fn moves_to_graveyard(part: &CostPart) -> bool {
    matches!(
        part,
        CostPart::Chosen(CostAction::Sacrifice | CostAction::Discard, _)
    )
}

pub(super) fn has_moving_parts(face: &Face, costs: Castings) -> bool {
    face.cost_parts(costs).any(moves_to_graveyard)
}

/// Pay sacrifices and discards before exiling cards. [CR 601.2h] lets a player pay
/// these costs "in any order" and says "Partial payments are not allowed."
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub(super) fn ordered_parts(
    face: &Face,
    costs: Castings,
) -> impl Iterator<Item = &'static CostPart> + '_ {
    face.cost_parts(costs)
        .filter(|part| moves_to_graveyard(part))
        .chain(
            face.cost_parts(costs)
                .filter(|part| !moves_to_graveyard(part)),
        )
}

fn collect_parts<L: Layout>(
    parts: impl Iterator<Item = &'static CostPart>,
) -> Result<L::Queue<CostPartRef, INLINE_COST_PARTS>, Full> {
    let mut ordered: L::Queue<CostPartRef, INLINE_COST_PARTS> = Default::default();
    for part in parts {
        ordered.push(CostPartRef(part))?;
    }
    Ok(ordered)
}

pub(super) fn payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    spell: L::Slot,
    face: &Face,
    costs: Castings,
) -> Result<bool, Full> {
    let parts = collect_parts::<L>(ordered_parts(face, costs))?;
    Payment { t, player, spell }.can_finish(s, parts.as_slice())
}

/// Check each offered pick against the rest of the payment before collapsing
/// interchangeable objects. A token can pay a sacrifice but supplies no card to
/// exile. The same applies to a card moved elsewhere by a replacement effect.
pub(super) fn completable_choices<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    player: Seat,
    paying: &PayingObjects<L>,
    able: Bitboard<L::Slot>,
) -> Result<Bitboard<L::Slot>, Full> {
    let StackItem::Spell {
        card, face, costs, ..
    } = entry.item
    else {
        return Ok(able);
    };
    let face = t.def(card).face(face);
    if !has_moving_parts(face, costs) {
        return Ok(able);
    }
    let parts = collect_parts::<L>(ordered_parts(face, costs).skip(usize::from(paying.part)))?;
    let Some((&CostPartRef(part @ CostPart::Chosen(_, chosen)), tail)) =
        parts.as_slice().split_first()
    else {
        unreachable!("the casting is paying a chosen part");
    };
    if tail.is_empty() {
        return Ok(able);
    }
    let payment = Payment {
        t,
        player,
        spell: itself(entry),
    };
    let mut completable = Bitboard::EMPTY;
    for &slot in s.in_game_order(t, able)?.as_slice() {
        let mut next = *paying;
        next.left -= 1;
        if chosen.zone == Zone::Graveyard {
            remember_cost_pick(s.graveyard(player), &mut next, slot);
        } else {
            next.chosen.insert(slot);
        }
        let rest = able & !next.chosen;
        let can_finish = if moves_to_graveyard(part) {
            payment.choose_moving(s, part, rest, &next, tail)?
        } else {
            let rest = if chosen.zone == Zone::Graveyard {
                let graveyard = s.graveyard(player);
                graveyard[graveyard_start(graveyard, &next)..]
                    .iter()
                    .copied()
                    .filter(|&slot| rest.contains(slot))
                    .collect()
            } else {
                rest
            };
            payment.remaining_fit(
                s,
                tail,
                Some((chosen.zone, u32::from(next.left), rest)),
                next.chosen,
            )?
        };
        if can_finish {
            completable.insert(slot);
        }
    }
    Ok(completable)
}

struct Payment<'a, L: Layout> {
    t: &'a GameTable,
    player: Seat,
    spell: L::Slot,
}

impl<L: Layout> Payment<'_, L> {
    fn can_finish(&self, s: &State<L>, parts: &[CostPartRef]) -> Result<bool, Full> {
        if let Some((&CostPartRef(part @ CostPart::Chosen(action, chosen)), tail)) =
            parts.split_first()
            && moves_to_graveyard(part)
        {
            let Measure::Count(n) = chosen.measure else {
                unreachable!("a spell's sacrifice or discard is counted");
            };
            let able = candidates(s, self.t, self.player, self.spell, *action, chosen);
            return self.choose_moving(s, part, able, &PayingObjects::start(0, n), tail);
        }
        self.remaining_fit(s, parts, None, Bitboard::EMPTY)
    }

    /// Search combinations of the current part's picks. Try the actual move on
    /// a scratch state so ownership, merged cards and replacement effects match
    /// what the real payment will do.
    fn choose_moving(
        &self,
        s: &State<L>,
        part: &CostPart,
        mut able: Bitboard<L::Slot>,
        picks: &PayingObjects<L>,
        tail: &[CostPartRef],
    ) -> Result<bool, Full> {
        if able.count() < u32::from(picks.left) {
            return Ok(false);
        }
        if tail.is_empty() {
            return Ok(true);
        }
        if picks.left == 0 {
            if picks.chosen.is_empty() {
                return self.can_finish(s, tail);
            }
            let CostPart::Chosen(action, chosen) = *part else {
                unreachable!("a sacrifice or discard chooses objects");
            };
            let mut paid = s.clone();
            crate::coverage::without_recording(|| {
                pay_with_all(
                    &mut paid,
                    self.t,
                    &mut Sink::off(),
                    action,
                    &chosen,
                    self.player,
                    picks.chosen,
                )
            })?;
            return self.can_finish(&paid, tail);
        }
        for &slot in s.in_game_order(self.t, able)?.as_slice() {
            able.remove(slot);
            let mut next = *picks;
            next.chosen.insert(slot);
            next.left -= 1;
            if self.choose_moving(s, part, able, &next, tail)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn remaining_fit(
        &self,
        s: &State<L>,
        parts: &[CostPartRef],
        current: Option<(Zone, u32, Bitboard<L::Slot>)>,
        picked: Bitboard<L::Slot>,
    ) -> Result<bool, Full> {
        let mut counted: L::Queue<(Zone, u32, Bitboard<L::Slot>), INLINE_COST_PARTS> =
            Default::default();
        if let Some(current) = current {
            counted.push(current)?;
        }
        for &CostPartRef(part) in parts {
            match part {
                CostPart::Chosen(action, chosen) => {
                    let Measure::Count(n) = chosen.measure else {
                        unreachable!("a spell's chosen costs are counted");
                    };
                    let able =
                        candidates(s, self.t, self.player, self.spell, *action, chosen) & !picked;
                    counted.push((chosen.zone, u32::from(n), able))?;
                }
                CostPart::Life(n) if !life::can_pay(s, self.t, self.player, *n) => {
                    return Ok(false);
                }
                CostPart::Life(_) => {}
                CostPart::This(_)
                | CostPart::Loyalty(_)
                | CostPart::OneOf(_)
                | CostPart::DiscardHand => {
                    unreachable!("unsupported spell cost");
                }
            }
        }
        Ok(costs_fit(counted.as_slice()))
    }
}

/// Each set of parts that take objects from one zone must have enough distinct
/// objects in their combined candidates to cover all those parts (Hall's theorem).
pub(super) fn costs_fit<S: SlotId>(counted: &[(Zone, u32, Bitboard<S>)]) -> bool {
    counted
        .iter()
        .enumerate()
        .all(|(index, &(zone, needed, cards))| {
            subsets_fit(&counted[index + 1..], zone, needed, cards)
        })
}

fn subsets_fit<S: SlotId>(
    tail: &[(Zone, u32, Bitboard<S>)],
    zone: Zone,
    needed: u32,
    cards: Bitboard<S>,
) -> bool {
    cards.count() >= needed
        && tail
            .iter()
            .enumerate()
            .all(|(index, &(other_zone, more, others))| {
                other_zone != zone
                    || subsets_fit(&tail[index + 1..], zone, needed + more, cards | others)
            })
}
