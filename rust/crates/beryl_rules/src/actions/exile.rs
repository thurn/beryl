//! Exile ([CR 701.13](https://yawgatog.com/resources/magic-rules/#R70113)).

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::Zone;
use crate::{stack, zones};

/// Exiles a set of objects simultaneously
/// ([CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a)): "To exile an object,
/// move it to the exile zone from wherever it is." A spell among them leaves the stack
/// without resolving.
pub fn exile<L: Layout>(game: &mut Game<'_, L>, objects: Bitboard<L::Slot>) -> Result<(), Full> {
    let rest = stack::unstack(game, objects, Zone::Exile)?;
    zones::move_all(game, rest, Zone::Exile, Reason::Exile)
}

/// [`Effect::Exile`](beryl_card::Effect::Exile): "Exile [objects]" (CR 701.13a). The outcome
/// is the cards that are in exile.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, Zone::Exile);
    if objects.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(objects);
    exile(run.game(), objects)?;
    Ok(run.now_in(objects, Zone::Exile))
}
