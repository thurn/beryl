//! Checks that mana payments leave enough permanents to pay a spell's other costs.
//!
//! Implements: none. Checks the complete payment required by the enclosing casting module.

use crate::bitboard::Bitboard;
use crate::card::{Castings, CostAction, CostPart, Face, FaceIx, ManaCost};
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::mana::{self, Paying, Plan};
use crate::replacement::event::Sink;
use crate::state::{StackEntry, StackItem, State};
use crate::table::{Capability, GameTable};

use super::parts;

/// A spell whose other costs can use one of the permanents that pays its mana.
/// See [CR 601.2g], "Mana abilities must be activated before costs are paid," and
/// [CR 601.2h], "Partial payments are not allowed."
///
/// [CR 601.2g]: https://yawgatog.com/resources/magic-rules/#R6012g
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub(super) struct SacrificeCost<L: Layout> {
    player: Seat,
    slot: L::Slot,
    face: &'static Face,
    costs: Castings,
}

impl<L: Layout> SacrificeCost<L> {
    fn of(player: Seat, slot: L::Slot, face: &'static Face, costs: Castings) -> Option<Self> {
        if face.additional_cost.is_empty() && costs.is_empty() {
            return None;
        }
        face.cost_parts(costs)
            .any(|part| matches!(part, CostPart::Chosen(CostAction::Sacrifice, _)))
            .then_some(Self {
                player,
                slot,
                face,
                costs,
            })
    }

    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "reads the face of the spell being paid for"
    )]
    pub(super) fn of_entry(t: &GameTable, entry: &StackEntry<L>) -> Option<Self> {
        if !t.can(Capability::SacrificeToCast) {
            return None;
        }
        let StackItem::Spell {
            card, face, costs, ..
        } = entry.item
        else {
            return None;
        };
        Self::of(
            entry.controller(),
            entry.spell()?.slot,
            t.def(card).face(face),
            costs,
        )
    }

    pub(super) fn payable(
        &self,
        s: &State<L>,
        t: &GameTable,
        mana: ManaCost,
        paying: Paying<L>,
    ) -> Result<bool, Full> {
        if !self.rest_is_payable(s, t)? {
            return Ok(false);
        }
        if mana.is_zero() {
            return Ok(true);
        }
        Ok(!self
            .plans(s, t, &mut Sink::off(), mana, true, paying)?
            .is_empty())
    }

    fn rest_is_payable(&self, s: &State<L>, t: &GameTable) -> Result<bool, Full> {
        parts::spell_payable(s, t, self.player, self.slot, self.face, self.costs)
    }

    fn plans(
        &self,
        s: &State<L>,
        t: &GameTable,
        sink: &mut Sink<L>,
        cost: ManaCost,
        greedy: bool,
        paying: Paying<L>,
    ) -> Result<L::Queue<Plan<L>, 64>, Full> {
        mana::plans_accepting::<L, true>(
            s,
            t,
            sink,
            self.player,
            cost,
            Bitboard::EMPTY,
            greedy,
            paying,
            |plan| {
                let mut after = s.clone();
                crate::coverage::without_recording(|| {
                    let mut sink = Sink::off();
                    mana::pay(
                        &mut after,
                        t,
                        &mut sink,
                        self.player,
                        cost,
                        Bitboard::EMPTY,
                        plan,
                        paying,
                    )?;
                    crate::continuous::layers::refresh(&mut after, t, &mut sink)
                })?;
                self.rest_is_payable(&after, t)
            },
        )
    }
}

/// Checks the full cost in a game that can ask for a sacrifice when casting.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn whole_cost_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    face_index: FaceIx,
    face: &'static Face,
    costs: Castings,
    mana: ManaCost,
) -> Result<bool, Full> {
    match SacrificeCost::of(player, slot, face, costs) {
        Some(other) => other.payable(s, t, mana, Paying::casting(s, t, slot, face_index)),
        None => parts::spell_payable(s, t, player, slot, face, costs),
    }
}

/// The plans for a spell must leave its other costs payable. An ability has already paid
/// its other costs by this point.
#[inline]
pub(super) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    entry: &StackEntry<L>,
    cost: ManaCost,
    greedy: bool,
) -> Result<L::Queue<Plan<L>, 64>, Full> {
    let paying = Paying::entry(s, t, entry);
    match SacrificeCost::of_entry(t, entry) {
        Some(other) => other.plans(s, t, sink, cost, greedy, paying),
        None => mana::plans(
            s,
            t,
            sink,
            entry.controller(),
            cost,
            Bitboard::EMPTY,
            greedy,
            paying,
        ),
    }
}

/// A manual payment may activate a mana ability only if the rest can still be paid.
pub(super) fn options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    remaining: ManaCost,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let player = entry.controller();
    let paying = Paying::entry(s, t, entry);
    let options = mana::manual_options(s, t, player, remaining, Bitboard::EMPTY, paying)?;
    let Some(other) = SacrificeCost::of_entry(t, entry) else {
        return Ok(options);
    };
    let mut allowed: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for &option in options.iter() {
        let mut after = s.clone();
        let rest = crate::coverage::without_recording(|| {
            let mut sink = Sink::off();
            let rest =
                mana::manual_pay(&mut after, t, &mut sink, player, remaining, option, paying)?;
            crate::continuous::layers::refresh(&mut after, t, &mut sink)?;
            Ok(rest)
        })?;
        if other.payable(&after, t, rest, paying)? {
            allowed.push(option)?;
        }
    }
    Ok(allowed)
}
