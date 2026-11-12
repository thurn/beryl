//! Search ([CR 701.23](https://yawgatog.com/resources/magic-rules/#R70123)).

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::filter::{Evaluate, Filter};
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{Zone, ZoneKey};
use crate::zones;

/// `player` searches their library for a card that passes `filter`, and finds one of the
/// cards that do, or none ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)):
/// "To search for a card in a zone, look at all cards in that zone (even if it's a hidden
/// zone) and find a card that matches the given description."
/// [CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "If a player is
/// searching a hidden zone for cards with a stated quality ... that player isn't required
/// to find some or all of those cards even if they're present in that zone." Returns the
/// card found, still in the library: the caller says where it goes, whether it is revealed
/// ([CR 701.23e](https://yawgatog.com/resources/magic-rules/#R70123e)), and shuffles.
/// The question is a [`DecisionKind::ChooseUpTo`](crate::decision::DecisionKind::ChooseUpTo)
/// whose [`Opt::Done`](crate::decision::Opt::Done) finds nothing.
pub fn search_library<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    filter: Filter,
) -> Result<Option<L::Slot>, Stop> {
    Ok(search_library_up_to(game, resolution, player, filter, 1)?.sole())
}

/// [`search_library`] for up to `n` cards, as "a basic land card" for each of several: those
/// found, still in the library, one `ChooseUpTo` at a time. The caller shuffles once they
/// have moved.
pub fn search_library_up_to<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    filter: Filter,
    n: u8,
) -> Result<Bitboard<L::Slot>, Stop> {
    let library = game.state.zone(ZoneKey::Of(player, Zone::Library));
    zones::look_at(game, player, library);
    let found = filter.select(game.state, game.table, library, player);
    let prompt = Prompt::new(Purpose::Search).most(n);
    resolution.choose_up_to(game, player, found, n, prompt)
}
