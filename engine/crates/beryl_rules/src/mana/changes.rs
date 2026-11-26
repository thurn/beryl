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
use crate::table::GameTable;
use beryl_card::{Added, Behavior, ManaChange, ManaColor, Tapped, Words};
use beryl_kernel::reference::Permanent;

use super::KINDS;

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

/// Whether any live mana-changing effect matches tapping `slot` for mana.
#[cold]
#[inline(never)]
pub(super) fn matches_any<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        if let ManaChange::Adds {
            tapped: Tapped::A(filter),
            added: Added::Of(_),
        } = *change
        {
            let words = Words::of_changes(effect.colors, effect.lands);
            if filter.matches(s, t, slot, effect.controller, &words) {
                return true;
            }
        }
    }
    false
}

/// Total additional fixed mana of each kind contributed by all live mana-changing effects when
/// `slot` is tapped for mana ([CR 106.12]): "To 'tap [a permanent] for mana' is to activate a
/// mana ability of that permanent that includes the {T} symbol in its activation cost."
///
/// [CR 106.12]: https://yawgatog.com/resources/magic-rules/#R10612
#[cold]
#[inline(never)]
pub(super) fn matching_added<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Result<[u8; KINDS], Full> {
    let mut extra = [0u8; KINDS];
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        let ManaChange::Adds {
            tapped: Tapped::A(filter),
            added: Added::Of(mana),
        } = *change
        else {
            continue;
        };
        let words = Words::of_changes(effect.colors, effect.lands);
        if filter.matches(s, t, slot, effect.controller, &words) {
            let k = mana.colors.trailing_zeros() as usize;
            extra[k] = extra[k].checked_add(mana.amount).ok_or(Full)?;
        }
    }
    Ok(extra)
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

/// Appends non-zero entries of `extra` onto `terms`, merging into an existing non-uniform
/// single-kind term of the same color where present.
pub(super) fn append_extra_mana(
    terms: &mut [super::rich::EvaluatedTerm; 8],
    count: &mut usize,
    extra: [u8; KINDS],
) -> Result<(), Full> {
    for (k, &n) in extra.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let mask = 1u8 << k;
        if let Some(existing) = terms[..*count]
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
