//! Phasing ([CR 702.26]): an effect that phases permanents out, and the untap step's phasing.
//!
//! The kernel's mutators ([`State::phase_out`], [`State::phase_in`]) do the work: a phased-out
//! permanent stays on the battlefield by its zone and leaves every battlefield list, so the rules
//! need nothing else to treat it as though it does not exist.
//!
//! [CR 702.26]: https://yawgatog.com/resources/magic-rules/#R70226

use beryl_card::Objects;

use crate::event::Sink;
use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layers;
use crate::layout::Layout;
use crate::state::State;
use crate::table::GameTable;

/// [`Effect::PhaseOut`](beryl_card::Effect::PhaseOut): "[Permanents] phase out"
/// ([CR 702.26b]): "If a permanent phases out, its status changes to “phased out.” Except for
/// rules and effects that specifically mention phased-out permanents, a phased-out permanent is
/// treated as though it does not exist." The outcome is the permanents that phased out directly.
///
/// Who controls each is read through the layers, which an instruction before may have changed,
/// so they are brought up to date first: that player's untap step phases it in.
///
/// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let permanents = run.permanents(objects);
    if permanents.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    let game = run.game();
    layers::refresh(game.state, game.table, game.sink)?;
    game.state.phase_out(permanents)?;
    Ok(Outcome::acted_on(permanents))
}

/// The phasing of the untap step, before anything untaps ([CR 502.1]): "First, all phased-in
/// permanents with phasing that the active player controls phase out, and all phased-out
/// permanents that the active player controlled when they phased out phase in. This all happens
/// simultaneously." One test of an empty set while nothing is phased out.
///
/// A permanent that phases in has been under its controller's control continuously since their
/// turn began, so it can attack and pay {T} costs this turn ([CR 302.6]): "A creature's activated
/// ability with the tap symbol or the untap symbol in its activation cost can't be activated
/// unless the creature has been under its controller's control continuously since their most
/// recent turn began."
///
/// No card in the game has the keyword phasing, so nothing phases out here yet.
///
/// [CR 502.1]: https://yawgatog.com/resources/magic-rules/#R5021
/// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
#[inline]
pub(crate) fn untap_step<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    if s.phased_out().is_empty() {
        return Ok(());
    }
    phase_in_for_active(s, t, sink)
}

/// The rare path of [`untap_step`], out of line.
#[cold]
#[inline(never)]
fn phase_in_for_active<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let active = s.turn().active();
    let phasing_in = s.phasing_in_for(active);
    if phasing_in.is_empty() {
        return Ok(());
    }
    s.phase_in(phasing_in)?;
    layers::refresh(s, t, sink)?;
    s.reset_permanents_for_turn(active);
    Ok(())
}
