//! Protection (see [CR 702.16]): what a permanent with protection from a quality is safe from.
//!
//! A permanent's protection is part of its layer results (`Derived::protection`), given in
//! layer 6 by the effects that grant it. Colors are the only quality the engine has yet
//! ([`beryl_card::Protection`]). Each of the four parts of the ability is read where the rule
//! it changes is:
//!
//! - damage (see [CR 702.16e]), a prevention effect applied to the damage proposal
//!   ([`crate::replacement::damage`]);
//! - targets ([CR 702.16b]), as they are chosen and again as the spell or ability resolves
//!   ([`crate::casting::targets`]'s candidates);
//! - Auras and Equipment (see [CR 702.16c] and see [CR 702.16d]), as state-based actions
//!   ([`crate::sba`]);
//! - blockers ([CR 702.16f]), as blocks are declared ([`crate::combat`]).
//!
//! While no effect gives protection, the layers leave [`Replaces::PROTECTION`] clear and each
//! test here is one AND ([`State::protection`]).
//!
//! Implements: CR 702.16a, CR 702.16b, CR 702.16f
//!
//! [CR 702.16]: https://yawgatog.com/resources/magic-rules/#R70216
//! [CR 702.16b]: https://yawgatog.com/resources/magic-rules/#R70216b
//! [CR 702.16c]: https://yawgatog.com/resources/magic-rules/#R70216c
//! [CR 702.16d]: https://yawgatog.com/resources/magic-rules/#R70216d
//! [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
//! [CR 702.16f]: https://yawgatog.com/resources/magic-rules/#R70216f
//! [`Replaces::PROTECTION`]: crate::replacement::event::Replaces::PROTECTION

use beryl_card::Colors;

use crate::bitboard::Bitboard;
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;
use beryl_kernel::reference::Permanent;

/// Whether any permanent may have protection now: the gate of every test here.
#[inline]
pub(crate) fn live<L: Layout>(s: &State<L>) -> bool {
    s.may_protect()
}

/// Whether `permanent` has protection from `source`: from one of the colors `source` has
/// ([CR 702.16a]): "This quality is usually a color (as in “protection from black”) but can be
/// any characteristic value or information." A source on the battlefield has the colors of its
/// layer results, and a spell or a card elsewhere those of its face ([`State::colors`]).
///
/// [CR 702.16a]: https://yawgatog.com/resources/magic-rules/#R70216a
#[inline]
pub(crate) fn protected_from<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    permanent: Permanent<L>,
    source: L::Slot,
) -> bool {
    let protection = s.protection(permanent);
    protection != Colors::NONE && protection.intersects(s.colors(t, source))
}

/// `objects` without the permanents that have protection from `source`, the spell or the
/// source of the ability choosing targets ([CR 702.16b]): "A permanent or player with
/// protection can’t be targeted by spells with the stated quality and can’t be targeted by
/// abilities from a source with the stated quality."
///
/// [CR 702.16b]: https://yawgatog.com/resources/magic-rules/#R70216b
#[inline]
pub(crate) fn targetable_by<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    objects: Bitboard<L::Slot>,
    source: Option<L::Slot>,
) -> Bitboard<L::Slot> {
    match source {
        Some(source) if live(s) => unprotected(s, t, objects, source),
        _ => objects,
    }
}

/// [`targetable_by`] while an effect gives protection, out of line.
#[cold]
#[inline(never)]
fn unprotected<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    objects: Bitboard<L::Slot>,
    source: L::Slot,
) -> Bitboard<L::Slot> {
    objects.filter(|slot| {
        !s.permanent_at(slot)
            .is_some_and(|permanent| protected_from(s, t, permanent, source))
    })
}

/// Whether `blocker` may not block `attacker` for the attacker's protection
/// ([CR 702.16f]): "Attacking creatures with protection can’t be blocked by creatures that
/// have the stated quality."
///
/// [CR 702.16f]: https://yawgatog.com/resources/magic-rules/#R70216f
#[inline]
pub(crate) fn forbids_block<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    attacker: L::Slot,
    blocker: L::Slot,
) -> bool {
    live(s)
        && s.permanent_at(attacker)
            .is_some_and(|attacker| protected_from_cold(s, t, attacker, blocker))
}

/// [`protected_from`], out of line, for the callers that have tested [`live`].
#[cold]
#[inline(never)]
pub(crate) fn protected_from_cold<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    permanent: Permanent<L>,
    source: L::Slot,
) -> bool {
    protected_from(s, t, permanent, source)
}
