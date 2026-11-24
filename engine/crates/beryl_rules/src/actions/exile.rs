//! Exile ([CR 701.13](https://yawgatog.com/resources/magic-rules/#R70113)).
//!
//! Implements: CR 701.13, CR 702.85a

use beryl_card::Objects;

use crate::bitboard::Bitboard;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::Reason;
use crate::state::ObjectRef;
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

type ExiledCards<L> = <L as Layout>::Queue<ObjectRef<L>, 128>;

/// Exiles a library's top cards one at a time, including the first one that matches
/// `stop` ([CR 702.85a]): "exile cards from the top of your library until you exile a
/// nonland card whose mana value is less than this spell’s mana value." The cards retain
/// their new identities in exile so later instructions can recognize them.
///
/// [CR 702.85a]: https://yawgatog.com/resources/magic-rules/#R70285a
pub(crate) fn from_top_until<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    stop: impl Fn(&Game<'_, L>, L::Slot) -> bool,
) -> Result<(ExiledCards<L>, Option<L::Slot>), Full> {
    let mut cards = ExiledCards::<L>::default();
    loop {
        let Some(slot) = game.state.library_cards_from_top(player).next() else {
            return Ok((cards, None));
        };
        let card = game.state.object_ref(slot);
        zones::move_to(game, card, Zone::Exile, Reason::Exile)?;
        if game.state.object(slot).zone() == Zone::Exile {
            cards.push(game.state.object_ref(slot))?;
            if stop(game, slot) {
                return Ok((cards, Some(slot)));
            }
        } else if game.state.is_current(card) && game.state.object(slot).zone() == Zone::Library {
            return Ok((cards, None));
        }
    }
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
    run.remember(objects)?;
    exile(run.game(), objects)?;
    Ok(run.now_in(objects, Zone::Exile))
}
