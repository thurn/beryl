//! Explore ([CR 701.44](https://yawgatog.com/resources/magic-rules/#R70144)).
//!
//! Implements: CR 701.44

use beryl_card::{Objects, prompt};

use super::reveal::reveal;
use crate::bitboard::Bitboard;
use crate::card::{Flow, Prompt, Purpose, Stop, Types};
use crate::game::Game;
use crate::interpret::choices;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::event::Reason;
use crate::resolution::Resolution;
use crate::state::{CounterKind, Zone};
use crate::{zones, zones::counters};
use beryl_kernel::reference::Permanent;

/// A permanent explores ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)):
/// "that permanent's controller reveals the top card of their library. If a land card is
/// revealed this way, that player puts that card into their hand. Otherwise, that player
/// puts a +1/+1 counter on the exploring permanent and may put the revealed card into
/// their graveyard."
///
/// [CR 701.44b](https://yawgatog.com/resources/magic-rules/#R70144b): it "explores ... even
/// if some or all of those actions were impossible": with an empty library no land is
/// revealed, so it gets the counter. [CR 701.44c](https://yawgatog.com/resources/magic-rules/#R70144c):
/// "If a permanent is no longer on the battlefield as an effect instructs it to explore,
/// its last known information is used to determine which object explored and who
/// controlled it." Nothing keeps who last controlled a permanent that has left, so that is
/// taken to be the controller of what is resolving, as it is for one "you control".
pub fn explore<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    permanent: Permanent<L>,
) -> Flow {
    let player = if game.state.permanent(permanent.into()).is_some() {
        game.state.controller(permanent.slot())
    } else {
        resolution.controller()
    };
    let top = game.state.library(player).last().copied();
    if let Some(slot) = top {
        reveal(game, player, slot);
        // A card in a library has its front face's characteristics (see CR 712.6a), and a
        // split card those of both halves (see CR 709.4c).
        if game
            .state
            .copiable(slot)
            .types(game.table)
            .contains(Types::LAND)
        {
            let card = game.state.object_ref(slot);
            zones::move_to(game, card, Zone::Hand, Reason::Effect)?;
            return Ok(());
        }
    }
    if let Some(permanent) = game.state().permanent(permanent.into()) {
        counters::add(game, permanent, CounterKind::PlusOne, 1)?;
    }
    // The card is revealed, on top of the library: a yes puts it into the graveyard.
    let prompt = Prompt::new(Purpose::Explore)
        .count(1)
        .places(prompt::Place::Graveyard, prompt::Place::Top);
    if let Some(slot) = top
        && choices::yes_no(game, resolution, player, prompt, Bitboard::single(slot))?
    {
        let card = game.state.object_ref(slot);
        zones::move_to(game, card, Zone::Graveyard, Reason::Effect)?;
    }
    Ok(())
}

/// [`Effect::Explore`](beryl_card::Effect::Explore): "[Permanents] explore" (CR 701.44a),
/// each in the order they entered the battlefield. The outcome is the permanents that
/// explored.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Stop> {
    let permanents = run.permanents(objects);
    let mut left = permanents;
    while let Some(object) = run.state().next_permanent(&mut left) {
        let (game, resolution) = run.resolving();
        explore(game, resolution, object)?;
    }
    Ok(Outcome::acted_on(permanents.slots()))
}
