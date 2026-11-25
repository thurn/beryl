//! A destruction proposed before it happens, and the replacement effect that modifies it:
//! regeneration ([CR 614.8]): "Regeneration is a destruction-replacement effect. The word
//! “instead” doesn't appear on the card but is implicit in the definition of regeneration."
//!
//! Implements: CR 614.8
//!
//! [CR 614.8]: https://yawgatog.com/resources/magic-rules/#R6148

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::layout::Layout;
use crate::replacement::event::{self, Sink};
use crate::state::State;
use crate::table::GameTable;
use beryl_card::Regeneration;
use beryl_kernel::reference::{Permanent, Permanents};

/// Of `permanents`, which a destruction would put into their owners' graveyards, those still
/// destroyed after its replacement effects: a regeneration shield saves each permanent it is
/// on ([`regenerate_any`]), unless `regeneration` forbids it (see [CR 701.19c]): effects that say a
/// permanent can't be regenerated "cause regeneration shields to not be applied." With no
/// shield on any permanent, one AND.
///
/// [CR 701.19c]: https://yawgatog.com/resources/magic-rules/#R70119c
#[inline]
pub(crate) fn destruction<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanents: Permanents<L>,
    regeneration: Regeneration,
) -> Result<Bitboard<L::Slot>, Full> {
    if regeneration == Regeneration::Forbidden || !s.has_regeneration_shields() {
        return Ok(permanents.slots());
    }
    Ok(permanents.slots() & !regenerate_any(s, t, sink, permanents)?)
}

/// Of `permanents`, which a destruction that may be replaced would destroy, regenerates
/// each that has a regeneration shield, in the order they entered the battlefield, and
/// returns those regenerated: the destruction leaves them where they are. With no shield on
/// any permanent, one AND for each.
fn regenerate_any<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanents: Permanents<L>,
) -> Result<Bitboard<L::Slot>, Full> {
    let shielded = (permanents.slots()).filter(|slot| s.is_shielded(s.object_ref(slot)));
    // In battlefield order, never slot order: the taps' events are seen in this order.
    let mut left = s.permanents_in(shielded);
    while let Some(permanent) = s.next_permanent(&mut left) {
        regenerate(s, t, sink, permanent)?;
    }
    Ok(shielded)
}

/// Uses up a shield on `permanent` in place of destroying it (see [CR 701.19a]): "The next time
/// [permanent] would be destroyed this turn, instead remove all damage marked on it and its
/// controller taps it. If it's an attacking or blocking creature, remove it from combat."
/// The tap is the one event: removing damage and leaving combat have none.
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
fn regenerate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
) -> Result<(), Full> {
    let used = s.use_regeneration_shield(permanent.into());
    debug_assert!(used, "a permanent regenerated with no shield");
    s.remove_damage(permanent);
    let o = s.object(permanent.slot());
    if o.is_attacking() || o.is_blocking() {
        s.remove_from_combat(permanent);
    }
    event::set_tapped(s, t, sink, permanent, true)
}
