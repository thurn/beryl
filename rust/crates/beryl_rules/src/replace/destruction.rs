//! A destruction proposed before it happens, and the replacement effect that modifies it:
//! regeneration ([CR 614.8]): "Regeneration is a destruction-replacement effect. The word
//! “instead” doesn't appear on the card but is implicit in the definition of regeneration."
//!
//! [CR 614.8]: https://yawgatog.com/resources/magic-rules/#R6148

use crate::bitboard::Bitboard;
use crate::event::{self, Sink};
use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::state::{ObjectRef, State};
use crate::table::GameTable;
use beryl_card::Regeneration;

/// Of `permanents`, which a destruction would put into their owners' graveyards, those still
/// destroyed after its replacement effects: a regeneration shield saves each permanent it is
/// on ([`regenerate_any`]), unless `regeneration` forbids it ([CR 701.19c]): effects that say a
/// permanent can't be regenerated "cause regeneration shields to not be applied." With no
/// shield on any permanent, one AND.
///
/// [CR 701.19c]: https://yawgatog.com/resources/magic-rules/#R70119c
#[inline]
pub(crate) fn destruction<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanents: Bitboard<L::Slot>,
    regeneration: Regeneration,
) -> Result<Bitboard<L::Slot>, Full> {
    if regeneration == Regeneration::Forbidden || !s.has_regeneration_shields() {
        return Ok(permanents);
    }
    Ok(permanents & !regenerate_any(s, t, sink, permanents)?)
}

/// Of `permanents`, which a destruction that may be replaced would destroy, regenerates
/// each that has a regeneration shield, in the order they entered the battlefield, and
/// returns those regenerated: the destruction leaves them where they are. With no shield on
/// any permanent, one AND for each.
fn regenerate_any<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanents: Bitboard<L::Slot>,
) -> Result<Bitboard<L::Slot>, Full> {
    let shielded = permanents.filter(|slot| s.is_shielded(s.object_ref(slot)));
    if shielded.is_empty() {
        return Ok(Bitboard::EMPTY);
    }
    // In battlefield order, never slot order: the taps' events are seen in this order.
    let ordered = s.in_game_order(t, shielded)?;
    for &slot in ordered.as_slice() {
        regenerate(s, t, sink, s.object_ref(slot))?;
    }
    Ok(shielded)
}

/// Uses up a shield on `object` in place of destroying it ([CR 701.19a]): "The next time
/// [permanent] would be destroyed this turn, instead remove all damage marked on it and its
/// controller taps it. If it's an attacking or blocking creature, remove it from combat."
/// The tap is the one event: removing damage and leaving combat have none.
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
fn regenerate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    object: ObjectRef<L>,
) -> Result<(), Full> {
    let used = s.use_regeneration_shield(object);
    debug_assert!(used, "a permanent regenerated with no shield");
    s.remove_damage(object);
    let o = s.object(object.slot);
    if o.is_attacking() || o.is_blocking() {
        s.remove_from_combat(object.slot);
    }
    event::set_tapped(s, t, sink, object, true)
}
