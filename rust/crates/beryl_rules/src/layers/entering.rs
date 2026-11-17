//! A permanent as it would exist on the battlefield, for the replacement effects that
//! modify how it enters ([CR 614.12]): "To determine which replacement effects apply and
//! how they apply, check the characteristics of the permanent as it would exist on the
//! battlefield, taking into account replacement effects that have already modified how it
//! enters the battlefield (see rule 616.1), continuous effects from the permanent’s own
//! static abilities that would apply to it once it’s on the battlefield, and continuous
//! effects that already exist and would apply to the permanent."
//!
//! [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412

use beryl_kernel::query::Copiable;

use crate::card::Face;
use crate::event::{self, Entry, Sink};
use crate::fixed::Full;
use crate::layout::Layout;
use crate::state::{Derived, State, Zone};
use crate::table::GameTable;
use crate::zone::Place;

/// The characteristics the object in `slot` would have on the battlefield if it entered as
/// `entry` says: under the controller and with the face `entry` names, with its own static
/// abilities and every continuous effect already in force that would apply to it.
///
/// The recompute runs on a copy of the state in which the object has entered, so the real
/// state is never changed and every effect finds the object as its layer comes up, as it
/// would find any permanent. The copy is made only for an object [`lost_entering`] says
/// may lose an ability, so most games never make one. Objects entering with it in one event are
/// left where they are: their effects do not exist yet ([`event::move_batch`]).
///
/// [`Full`] if the copy ran out of room as the object entered or as the layers applied, as
/// the real move would.
pub(crate) fn as_it_would_exist<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Derived<L>, Full> {
    entered(s, t, slot, entry, None, |copy| *copy.derived(slot))
}

/// What `read` finds in a copy of the state in which the object in `slot` has entered the
/// battlefield as `entry` says, with any chosen copy effect and the layers applied:
/// the copy [`as_it_would_exist`]
/// reads, for a caller that asks a filter of the object as it would exist on the
/// battlefield. The copy stays here, so it is never moved.
///
/// [`Full`] as [`as_it_would_exist`] gives it.
pub(crate) fn entered<L: Layout, R>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
    copied: Option<Copiable<L>>,
    read: impl FnOnce(&State<L>) -> R,
) -> Result<R, Full> {
    let mut copy = s.clone();
    let mut sink = Sink::off();
    copy.move_object(&mut sink, slot, Zone::Battlefield, Place::Top)?;
    event::arrive(&mut copy, t, slot, entry)?;
    if let Some(values) = copied {
        let permanent = copy.permanent_at(slot).expect("a copy just entered");
        copy.enter_as_copy(permanent, values)?;
    }
    super::refresh(&mut copy, t, &mut sink)?;
    Ok(read(&copy))
}

/// The abilities of `face` that the object in `slot` would not have if it entered as
/// `entry` says, with `face` up ([`as_it_would_exist`]), for a caller that has found an
/// entering replacement on that face. With no effect in force that removes abilities (the
/// layers store a flag for any such effect, whether or not it removes one from a permanent
/// now) and no static ability on the face that might, it loses none, and as most games go
/// that is one test of a flag.
///
/// [`Full`] as [`as_it_would_exist`] gives it.
pub(crate) fn lost_entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
    face: &Face,
) -> Result<L::AbilityMask, Full> {
    let may_differ = s.abilities_may_be_lost()
        || s.is_derived_stale()
        || face.abilities.iter().any(|a| a.layer().is_some());
    if !may_differ {
        return Ok(L::AbilityMask::default());
    }
    Ok(as_it_would_exist(s, t, slot, entry)?.lost)
}
