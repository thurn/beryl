//! Class levels and solved Cases.
//!
//! Implements: CR 717.2a, CR 720.3a, CR 720.3c

use beryl_card::Designation;
use beryl_kernel::derive::Rederive;

use crate::card::{Ability, AbilityIx};
use crate::continuous::layers;
use crate::fixed::Full;
use crate::layout::Layout;
use crate::replacement::event::{Event, Sink};
use crate::stack::trigger;
use crate::state::{AbilityMask, Memo, ObjectRef, State};
use crate::table::GameTable;

/// The Class abilities below a level bar are present from that level onward ([CR 717.2a]):
/// "As long as this Class is level N or greater, it has [abilities]." A Solved ability can
/// affect the game, trigger or be activated only if the Case "has the solved designation"
/// ([CR 720.3c]).
///
/// [CR 717.2a]: https://yawgatog.com/resources/magic-rules/#R7172a
/// [CR 720.3c]: https://yawgatog.com/resources/magic-rules/#R7203c
pub(crate) fn available<L: Layout>(s: &State<L>, slot: L::Slot, ability: &Ability) -> bool {
    match ability.designation() {
        Designation::AtLevel(level) => s.class_level(slot) >= level,
        Designation::Solved => s.is_solved(slot),
        Designation::None
        | Designation::Imprint
        | Designation::LevelUp(_)
        | Designation::ToSolve => true,
    }
}

/// Records unavailable abilities alongside the abilities missing from the face after layer
/// 1. Continuous effects still remove the remaining abilities in layer 6 (see [CR 613.1f]).
///
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
#[cold]
#[allow(
    clippy::disallowed_methods,
    reason = "the layers read a permanent's printed face up, by each ability's place on it"
)]
pub(crate) fn withhold<L: Layout>(s: &mut Rederive<'_, L>, t: &GameTable) -> Result<bool, Full> {
    let mut missing = false;
    for i in 0..s.battlefield().len() {
        let slot = s.battlefield()[i];
        let mut lost = s.rows()[i].lost;
        for (ability, index) in s.definition(slot).face(t).abilities.iter().zip(0..) {
            if !available(s, slot, ability) {
                lost = lost.with(AbilityIx::new(index)).ok_or(Full)?;
                missing = true;
            }
        }
        s.results()[i].lost = lost;
    }
    Ok(missing)
}

/// "[Cost]: This Class’s level becomes N. Activate only if this Class is level N-1 and
/// only as a sorcery" ([CR 717.2a]). An ability that triggers at the new level sees the
/// permanent's newly available abilities (see [CR 603.10]): "objects that exist immediately
/// after an event are checked to see if the event matched any trigger conditions".
/// The Case's effect makes the designation after its intervening condition was checked
/// ([CR 720.3a]): "if [condition] and this Case is not solved, this Case becomes solved."
///
/// [CR 717.2a]: https://yawgatog.com/resources/magic-rules/#R7172a
/// [CR 603.10]: https://yawgatog.com/resources/magic-rules/#R60310
/// [CR 720.3a]: https://yawgatog.com/resources/magic-rules/#R7203a
#[cold]
pub(crate) fn resolve<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    source: Option<ObjectRef<L>>,
    designation: Designation,
) -> Result<(), Full> {
    let Some(source) = source.filter(|&r| s.permanent(r).is_some()) else {
        return Ok(());
    };
    match designation {
        Designation::LevelUp(level) if s.class_level(source.slot) != level => {
            s.set_class_level(source.slot, level)?;
            layers::refresh(s, t, sink)?;
            trigger::match_event(
                s,
                t,
                &Event::ClassLevel {
                    object: source,
                    level,
                },
            )
        }
        Designation::ToSolve => {
            s.remember(source.slot, Memo::Solved)?;
            layers::refresh(s, t, sink)
        }
        Designation::None
        | Designation::LevelUp(_)
        | Designation::AtLevel(_)
        | Designation::Solved
        | Designation::Imprint => Ok(()),
    }
}
