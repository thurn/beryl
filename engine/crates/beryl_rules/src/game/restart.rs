//! Ends a game during resolution and finishes the carried cards' entry after mulligans.
//!
//! Implements: CR 728.1, CR 728.1a, CR 728.3, CR 728.4

use beryl_card::{FaceIx, Filter, Types};

use crate::bitboard::Bitboard;
use crate::card::{Flow, Stop};
use crate::fixed::Full;
use crate::game::start;
use crate::interpret::Run;
use crate::layout::Layout;
use crate::replacement::event::{self, Entry, Event, Reason, Sink};
use crate::state::{State, Zone};
use crate::table::GameTable;
use crate::turn;

/// Replaces the board and deals opening hands. Under [CR 728.1], "No players in that
/// game win, lose, or draw that game." Under [CR 728.1a], "The starting player in the
/// new game is the player who controlled the spell or ability that restarted the game."
/// A player who draws from an empty library during this deal loses at the first upkeep
/// under [CR 728.3]: "any player with fewer than seven cards in their library will lose
/// the game when state-based actions are checked during the upkeep step of the first turn".
///
/// [CR 728.1]: https://yawgatog.com/resources/magic-rules/#R7281
/// [CR 728.1a]: https://yawgatog.com/resources/magic-rules/#R7281a
/// [CR 728.3]: https://yawgatog.com/resources/magic-rules/#R7283
#[cold]
pub(crate) fn instruction<L: Layout>(run: &mut Run<'_, '_, L>, filter: Filter) -> Flow {
    let carried = run.scene().linked_exiles(filter);
    let (game, resolving) = run.resolving();
    let controller = resolving.controller();
    game.state.restart(game.table, controller, carried)?;
    game.sink.note(Event::GameRestarted);
    game.sink.revealed = Bitboard::EMPTY;
    start::set_starting_player(game.state, game.table, game.sink, controller)?;
    Err(Stop::RestartGame)
}

/// Finishes the restart under [CR 728.4]: "The effect that restarts the game finishes
/// resolving just before the first turn’s untap step." Entering abilities wait until the
/// first upkeep, since "No player has priority, and any triggered abilities that trigger
/// as a result will go on the stack the next time a player receives priority".
///
/// [CR 728.4]: https://yawgatog.com/resources/magic-rules/#R7284
#[allow(
    clippy::disallowed_methods,
    reason = "reads the front face of the card a face-down exiled object really is (see CR 712.6a)"
)]
pub(crate) fn finish_opening<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    // See CR 400.4a: "If an instant or sorcery card would enter the battlefield, it remains
    // in its previous zone."
    let carried = s.in_zone(Zone::Exile).filter(|slot| {
        t.def(s.card_really_is(slot))
            .face(FaceIx::FRONT)
            .types
            .intersects(Types::PERMANENT)
    });
    if !carried.is_empty() {
        let entry = Entry {
            controller: Some(s.turn().starting_player()),
            ..Entry::default()
        };
        event::move_batch(
            s,
            t,
            sink,
            carried,
            Zone::Battlefield,
            Reason::Effect,
            entry,
        )?;
    }
    turn::begin_turn(s, t, sink)
}
