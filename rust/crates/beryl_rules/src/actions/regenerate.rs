//! Regenerate ([CR 701.19]).
//!
//! A resolving "regenerate" creates a regeneration shield: an entry of the Effects store on
//! the permanent, lasting until end of turn. The shield is a replacement effect of
//! destruction ([CR 614.8]): "Regeneration is a destruction-replacement effect." Each
//! destruction that may be replaced asks [`regenerate_any`] which of the permanents it
//! would destroy a shield saves: a destruction by an effect ([`super::destroy`]) unless it
//! says they can't be regenerated, and the state-based destruction of a creature with
//! lethal damage (`sba`). A sacrifice and a creature with toughness 0 are not destroyed, so
//! no shield applies to them.
//!
//! [CR 701.19]: https://yawgatog.com/resources/magic-rules/#R70119
//! [CR 614.8]: https://yawgatog.com/resources/magic-rules/#R6148

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::event::{self, Sink};
use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::state::{Continuous, Duration, ObjectRef, State};
use crate::table::GameTable;

/// [`Effect::Regenerate`](beryl_card::Effect::Regenerate): "Regenerate [permanents]": a
/// shield on each of them ([CR 701.19a]): "If the effect of a resolving spell or ability
/// regenerates a permanent, it creates a replacement effect that protects the permanent the
/// next time it would be destroyed this turn." The outcome is the permanents shielded.
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    let mut left = permanents;
    while let Some(permanent) = run.next_permanent(&mut left) {
        let (game, resolution) = run.resolving();
        let shield = Continuous::Regeneration(permanent);
        crate::continuous::start(game, resolution, shield, Duration::EndOfTurn)?;
    }
    Ok(Outcome::acted_on(permanents))
}

/// Of `permanents`, which a destruction that may be replaced would destroy, regenerates
/// each that has a regeneration shield, in the order they entered the battlefield, and
/// returns those regenerated: the destruction leaves them where they are. With no shield on
/// any permanent, one AND for each.
pub(crate) fn regenerate_any<L: Layout>(
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
