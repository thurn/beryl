//! Live mana-changing effects ([`beryl_card::ManaChange`]) applied when a permanent is tapped
//! for mana.
//!
//! Implements: CR 106.12, CR 605.1b, CR 605.4a

use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::ids::{CardId, Seat};
use crate::layout::Layout;
use crate::replacement::event::Sink;
use crate::state::{ManaEffect, State};
use crate::table::{Capability, GameTable};
use beryl_card::{Added, Behavior, ManaChange, ManaColor, Tapped, Words};
use beryl_kernel::event::Replaces;
use beryl_kernel::reference::Permanent;

use super::KINDS;

/// Additional mana contributed by all live mana-changing effects and permanent abilities when
/// a permanent is tapped for mana.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MatchingChanges {
    /// Fixed additional mana of each kind ([`Added::Of`]).
    pub(super) fixed: [u8; KINDS],
    /// Count of triggered mana abilities that add one mana of any type the tap produced
    /// ([`Added::TypeProduced`]).
    pub(super) type_produced: u8,
}

impl MatchingChanges {
    /// Total extra mana when the tapped ability produced only `color`.
    pub(super) fn add_for_single_color(mut self, color: ManaColor) -> Result<[u8; KINDS], Full> {
        if self.type_produced > 0 {
            let k = color.index();
            self.fixed[k] = self.fixed[k].checked_add(self.type_produced).ok_or(Full)?;
        }
        Ok(self.fixed)
    }
}

/// Whether a permanent `source_slot`'s mana-change trigger (`tapped`) matches tapping `slot`.
fn permanent_matches<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    source_slot: L::Slot,
    slot: L::Slot,
    tapped: Tapped,
) -> bool {
    match tapped {
        Tapped::A(filter) => {
            let words = s.words_of(source_slot);
            let controller = s.controller(source_slot);
            filter.matches(s, t, slot, controller, &words)
        }
        Tapped::AttachedTo => s.object(source_slot).attached_to() == Some(slot),
    }
}

/// Looks up the [`ManaChange`] data referenced by a live [`ManaEffect`].
#[inline]
#[allow(
    clippy::disallowed_methods,
    reason = "reads the source card's printed face to look up the ChangeMana instruction data"
)]
pub(super) fn read_mana_change(
    t: &GameTable,
    source: impl CardId,
    effect: ManaEffect,
) -> &'static ManaChange {
    let face = t.def(source).face(effect.face);
    let ability = &face.abilities[effect.ability.index()];
    let Behavior::Data(root) = ability.body() else {
        unreachable!("ManaEffect points to a data ability");
    };
    root.mana_change_at(effect.part)
        .expect("ManaEffect part points to a ChangeMana instruction")
}

/// Whether any live mana-changing effect or permanent ability matches tapping `slot` for mana.
#[cold]
#[inline(never)]
pub(super) fn matches_any<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        if let ManaChange::Adds {
            tapped: Tapped::A(filter),
            ..
        } = *change
        {
            let words = Words::of_changes(effect.colors, effect.lands);
            if filter.matches(s, t, slot, effect.controller, &words) {
                return true;
            }
        }
    }
    if s.replacing().is_some_and(Replaces::has_mana_change) {
        let merges = t.can(Capability::Merges);
        for &source_slot in s.battlefield() {
            if !s.definition(source_slot).facts(t).has_mana_change && !merges {
                continue;
            }
            for (_, a) in s.abilities(t, source_slot) {
                if let Some(&ManaChange::Adds { tapped, .. }) = a.changes_mana()
                    && permanent_matches(s, t, source_slot, slot, tapped)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Total additional fixed mana and [`Added::TypeProduced`] triggers contributed by all live
/// mana-changing effects and permanent abilities when `slot` is tapped for mana ([CR 106.12]):
/// "To 'tap [a permanent] for mana' is to activate a mana ability of that permanent that includes
/// the {T} symbol in its activation cost."
///
/// [CR 106.12]: https://yawgatog.com/resources/magic-rules/#R10612
#[cold]
#[inline(never)]
pub(super) fn matching<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Result<MatchingChanges, Full> {
    let mut out = MatchingChanges::default();
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        let ManaChange::Adds {
            tapped: Tapped::A(filter),
            added,
        } = *change
        else {
            continue;
        };
        let words = Words::of_changes(effect.colors, effect.lands);
        if filter.matches(s, t, slot, effect.controller, &words) {
            record_added(&mut out, added)?;
        }
    }
    if s.replacing().is_some_and(Replaces::has_mana_change) {
        let merges = t.can(Capability::Merges);
        for &source_slot in s.battlefield() {
            if !s.definition(source_slot).facts(t).has_mana_change && !merges {
                continue;
            }
            for (_, a) in s.abilities(t, source_slot) {
                if let Some(&ManaChange::Adds { tapped, added }) = a.changes_mana()
                    && permanent_matches(s, t, source_slot, slot, tapped)
                {
                    crate::coverage::hit(a);
                    record_added(&mut out, added)?;
                }
            }
        }
    }
    Ok(out)
}

fn record_added(out: &mut MatchingChanges, added: Added) -> Result<(), Full> {
    match added {
        Added::Of(mana) => {
            let k = mana.colors.trailing_zeros() as usize;
            out.fixed[k] = out.fixed[k].checked_add(mana.amount).ok_or(Full)?;
        }
        Added::TypeProduced => {
            out.type_produced = out.type_produced.checked_add(1).ok_or(Full)?;
        }
    }
    Ok(())
}

/// The kinds of mana `slot`'s intrinsic or simple tap-for-mana abilities make when live mana
/// changes moved it out of `sources::source`, for standalone priority activations.
#[cold]
#[inline(never)]
pub(super) fn standalone_kinds<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    d: &crate::state::Derived<L>,
) -> u8 {
    if !super::sources::can_tap(s, player, slot, d) {
        return 0;
    }
    let mut kinds = s.intrinsic_mana(t, slot);
    let words = s.words_of(slot);
    for (_, a) in s.abilities(t, slot) {
        if crate::table::is_simple(a) && super::sources::allowed(s, t, a, player, &words) {
            kinds |= a.produces().colors;
        }
    }
    kinds
}

/// Appends non-zero entries of `extra` onto `terms` starting at `fixed_start`, merging into an
/// existing non-uniform single-kind term in `terms[fixed_start..*count]` where present.
pub(super) fn append_extra_mana(
    terms: &mut [super::rich::EvaluatedTerm; 8],
    fixed_start: usize,
    count: &mut usize,
    extra: [u8; KINDS],
) -> Result<(), Full> {
    for (k, &n) in extra.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let mask = 1u8 << k;
        if let Some(existing) = terms[fixed_start..*count]
            .iter_mut()
            .find(|t| t.kinds == mask && !t.uniform)
        {
            existing.amount = existing.amount.checked_add(n).ok_or(Full)?;
        } else {
            if *count >= terms.len() {
                return Err(Full);
            }
            terms[*count] = super::rich::EvaluatedTerm {
                kinds: mask,
                amount: n,
                uniform: false,
            };
            *count += 1;
        }
    }
    Ok(())
}

/// Adds `extra` mana from live mana-changing effects ([CR 605.1b]) to `player`'s pool in the
/// current event group ([CR 605.4a]): "A triggered mana ability doesn't go on the stack, so it
/// can't be targeted, countered, or otherwise responded to. Rather, it resolves immediately
/// after the mana ability that triggered it, without waiting for priority."
///
/// [CR 605.1b]: https://yawgatog.com/resources/magic-rules/#R6051b
/// [CR 605.4a]: https://yawgatog.com/resources/magic-rules/#R6054a
#[cold]
#[inline(never)]
pub(super) fn add_extra<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    player: Seat,
    extra: [u8; KINDS],
) {
    let source = Some(permanent.into());
    for (k, &n) in extra.iter().enumerate() {
        if n > 0 {
            let added = s.add_mana(player, source, ManaColor::ALL[k], n);
            sink.note_from(source, added);
        }
    }
}
