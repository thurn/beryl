//! Live mana-changing effects ([`beryl_card::ManaChange`]) applied when a permanent is tapped
//! for mana, and the types of mana a permanent could produce ([CR 106.7]).
//!
//! Implements: CR 106.7, CR 106.12, CR 605.1b, CR 605.4a
//!
//! [CR 106.7]: https://yawgatog.com/resources/magic-rules/#R1067

use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::ids::{CardId, Seat};
use crate::layout::Layout;
use crate::replacement::event::Sink;
use crate::state::{ManaEffect, State};
use crate::table::{Capability, GameTable, is_could_produce};
use beryl_card::{
    Ability, AbilityKind, Added, Amount, Behavior, Filter, Kinds, ManaChange, ManaColor, Tapped,
    Term, Words,
};
use beryl_kernel::event::Replaces;
use beryl_kernel::reference::Permanent;

use super::KINDS;

/// Additional mana and multipliers contributed by all live mana-changing effects and permanent
/// abilities when a permanent is tapped for mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MatchingChanges {
    /// Fixed additional mana of each kind ([`Added::Of`]).
    pub(super) fixed: [u8; KINDS],
    /// Count of triggered mana abilities that add one mana of any type the tap produced
    /// ([`Added::TypeProduced`]).
    pub(super) type_produced: u8,
    /// Product of all matching [`ManaChange::Multiplies`] replacement effects (1 when none).
    pub(super) multiplier: u8,
}

impl Default for MatchingChanges {
    fn default() -> Self {
        Self {
            fixed: [0; KINDS],
            type_produced: 0,
            multiplier: 1,
        }
    }
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
    let tapped_controller = s.controller(slot);
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        let words = Words::of_changes(effect.colors, effect.lands);
        match *change {
            ManaChange::Adds {
                tapped: Tapped::A(filter),
                ..
            } if filter.matches(s, t, slot, effect.controller, &words) => return true,
            ManaChange::Multiplies { tapped, .. }
                if tapped_controller == effect.controller
                    && tapped.matches(s, t, slot, effect.controller, &words) =>
            {
                return true;
            }
            ManaChange::Adds { .. } | ManaChange::Multiplies { .. } => {}
        }
    }
    if s.replacing().is_some_and(Replaces::has_mana_change) {
        let merges = t.can(Capability::Merges);
        for &source_slot in s.battlefield() {
            if !s.definition(source_slot).facts(t).has_mana_change && !merges {
                continue;
            }
            let source_controller = s.controller(source_slot);
            for (_, a) in s.abilities(t, source_slot) {
                match a.changes_mana() {
                    Some(&ManaChange::Adds { tapped, .. })
                        if permanent_matches(s, t, source_slot, slot, tapped) =>
                    {
                        return true;
                    }
                    Some(&ManaChange::Multiplies { tapped, .. })
                        if tapped_controller == source_controller
                            && tapped.matches(
                                s,
                                t,
                                slot,
                                source_controller,
                                &s.words_of(source_slot),
                            ) =>
                    {
                        return true;
                    }
                    Some(&ManaChange::Adds { .. } | &ManaChange::Multiplies { .. }) | None => {}
                }
            }
        }
    }
    false
}

/// Total additional fixed mana, [`Added::TypeProduced`] triggers, and multipliers contributed by
/// all live mana-changing effects and permanent abilities when `slot` is tapped for mana
/// ([CR 106.12]): "To 'tap [a permanent] for mana' is to activate a mana ability of that
/// permanent that includes the {T} symbol in its activation cost."
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
    let tapped_controller = s.controller(slot);
    for (card, effect) in s.mana_effects() {
        let change = read_mana_change(t, card, effect);
        let words = Words::of_changes(effect.colors, effect.lands);
        match *change {
            ManaChange::Adds {
                tapped: Tapped::A(filter),
                added,
            } if filter.matches(s, t, slot, effect.controller, &words) => {
                record_added(&mut out, added)?;
            }
            ManaChange::Multiplies { tapped, by }
                if tapped_controller == effect.controller
                    && tapped.matches(s, t, slot, effect.controller, &words) =>
            {
                out.multiplier = out.multiplier.checked_mul(by).ok_or(Full)?;
            }
            ManaChange::Adds { .. } | ManaChange::Multiplies { .. } => {}
        }
    }
    if s.replacing().is_some_and(Replaces::has_mana_change) {
        let merges = t.can(Capability::Merges);
        for &source_slot in s.battlefield() {
            if !s.definition(source_slot).facts(t).has_mana_change && !merges {
                continue;
            }
            let source_controller = s.controller(source_slot);
            for (_, a) in s.abilities(t, source_slot) {
                match a.changes_mana() {
                    Some(&ManaChange::Adds { tapped, added })
                        if permanent_matches(s, t, source_slot, slot, tapped) =>
                    {
                        crate::coverage::hit(a);
                        record_added(&mut out, added)?;
                    }
                    Some(&ManaChange::Multiplies { tapped, by })
                        if tapped_controller == source_controller
                            && tapped.matches(
                                s,
                                t,
                                slot,
                                source_controller,
                                &s.words_of(source_slot),
                            ) =>
                    {
                        crate::coverage::hit(a);
                        out.multiplier = out.multiplier.checked_mul(by).ok_or(Full)?;
                    }
                    Some(&ManaChange::Adds { .. } | &ManaChange::Multiplies { .. }) | None => {}
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
            if is_could_produce(a) {
                kinds |= could_produce_of(s, t, player, a, &words);
            } else {
                kinds |= a.produces().colors;
            }
        }
    }
    kinds
}

/// Whether `player` may activate a mana ability now (see
/// [CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin to
/// activate an ability that's prohibited from being activated." Read through `words`.
pub(super) fn allowed<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    a: &Ability,
    player: Seat,
    words: &Words,
) -> bool {
    a.only_if_you_control().is_none_or(|f| {
        s.battlefield()
            .iter()
            .any(|&slot| s.controller(slot) == player && f.matches(s, t, slot, player, words))
    })
}

/// Evaluates the [`Kinds::CouldProduce`] filter of a single-term ability `a` ([`is_could_produce`]).
#[inline]
pub(super) fn could_produce_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    controller: Seat,
    a: &Ability,
    words: &Words,
) -> u8 {
    match a.product() {
        [
            Term {
                kinds: Kinds::CouldProduce(filter),
                ..
            },
        ] => could_produce(s, t, controller, filter, words),
        _ => 0,
    }
}

/// The union of mana types that permanents matching `filter` (evaluated for `controller` with
/// `words`) could produce ([CR 106.7]), ignoring costs and tapped state and reading only
/// abilities of the permanents themselves (not `ManaChange::Adds` triggers). Abilities whose
/// own product is [`Kinds::CouldProduce`] start empty and are iterated to their least fixed
/// point in at most six rounds.
///
/// [CR 106.7]: https://yawgatog.com/resources/magic-rules/#R1067
#[cold]
#[inline(never)]
pub(super) fn could_produce<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    controller: Seat,
    filter: &Filter,
    words: &Words,
) -> u8 {
    let battlefield = s.battlefield();
    let mut known = vec![0u8; battlefield.len()];
    let mut has_dynamic = false;
    for (i, &slot) in battlefield.iter().enumerate() {
        let mut kinds = s.intrinsic_mana(t, slot);
        for (_, a) in s.abilities(t, slot) {
            if a.kind() != AbilityKind::Mana {
                continue;
            }
            for term in a.product() {
                let amount = if let Amount::Fixed(n) = term.amount {
                    n
                } else {
                    crate::interpret::amount_of(s, t, slot, term.amount).unwrap_or(1)
                };
                if amount == 0 {
                    continue;
                }
                match term.kinds {
                    Kinds::Fixed(bits) => kinds |= bits,
                    Kinds::CouldProduce(_) => has_dynamic = true,
                }
            }
        }
        known[i] = kinds;
    }
    if has_dynamic {
        for _ in 0..KINDS {
            let mut changed = false;
            for (i, &slot) in battlefield.iter().enumerate() {
                let mut added = 0u8;
                let slot_controller = s.controller(slot);
                let slot_words = s.words_of(slot);
                for (_, a) in s.abilities(t, slot) {
                    if a.kind() != AbilityKind::Mana {
                        continue;
                    }
                    for term in a.product() {
                        let Kinds::CouldProduce(sub_filter) = term.kinds else {
                            continue;
                        };
                        let amount = if let Amount::Fixed(n) = term.amount {
                            n
                        } else {
                            crate::interpret::amount_of(s, t, slot, term.amount).unwrap_or(1)
                        };
                        if amount == 0 {
                            continue;
                        }
                        for (j, &other) in battlefield.iter().enumerate() {
                            if known[j] != 0
                                && sub_filter.matches(s, t, other, slot_controller, &slot_words)
                            {
                                added |= known[j];
                            }
                        }
                    }
                }
                if known[i] | added != known[i] {
                    known[i] |= added;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    let mut out = 0u8;
    for (j, &slot) in battlefield.iter().enumerate() {
        if known[j] != 0 && filter.matches(s, t, slot, controller, words) {
            out |= known[j];
        }
    }
    out
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
) -> Result<(), Full> {
    let source = Some(permanent.into());
    for (k, &n) in extra.iter().enumerate() {
        if n > 0 {
            let added = s.add_mana(player, source, ManaColor::ALL[k], n)?;
            sink.note_from(source, added);
        }
    }
    Ok(())
}
