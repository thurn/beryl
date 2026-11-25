//! Preparation cards and their linked copies in exile. Under [CR 723.3c], "This copy
//! remains in exile for as long as the prepared permanent remains on the battlefield
//! and has the prepared designation."
//!
//! [CR 723.3c]: https://yawgatog.com/resources/magic-rules/#R7233c
//!
//! Implements: CR 723.3a, CR 723.3b, CR 723.3c

use beryl_card::Objects;
use beryl_kernel::reference::Permanent;

use crate::bitboard::Bitboard;
use crate::continuous::layers;
use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::GameTable;

/// Designates a permanent as prepared and creates its prepare spell copy in exile.
/// Under [CR 723.3a], "A permanent can’t gain this designation unless it has a prepare spell.
/// Additionally, a permanent can’t gain this designation if the permanent already has it."
///
/// [CR 723.3a]: https://yawgatog.com/resources/magic-rules/#R7233a
pub(crate) fn prepare<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
) -> Result<bool, Full> {
    let slot = permanent.slot();
    if s.is_prepared(slot) {
        return Ok(false);
    }
    s.create_prepare_copy(t, sink, slot)
}

/// Applies "becomes prepared" or "becomes unprepared" to the chosen permanents.
/// Under [CR 723.3b], "A rule or effect may cause a permanent to become “unprepared.” This
/// removes the prepared designation from that permanent."
///
/// [CR 723.3b]: https://yawgatog.com/resources/magic-rules/#R7233b
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    prepared: bool,
) -> Result<Outcome<L>, Full> {
    let mut permanents = run.permanents(objects);
    let game = run.game();
    layers::refresh(game.state, game.table, game.sink)?;
    let mut changed = Bitboard::EMPTY;
    while let Some(permanent) = game.state.next_permanent(&mut permanents) {
        let slot = permanent.slot();
        let did = if prepared {
            prepare(game.state, game.table, game.sink, permanent)?
        } else {
            game.state.unprepare(slot)?
        };
        if did {
            changed.insert(slot);
        }
    }
    Ok(Outcome::acted_on(changed))
}
