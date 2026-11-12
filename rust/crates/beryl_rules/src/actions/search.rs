//! Search ([CR 701.23](https://yawgatog.com/resources/magic-rules/#R70123)).

use beryl_card::{Amount, Found, Players};

use crate::actions::{reveal, shuffle};
use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::choices;
use crate::event::Reason;
use crate::filter::{Evaluate, Filter};
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
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
    choices::pick_up_to(game, resolution, player, found, n, prompt)
}

/// [`Effect::Search`](beryl_card::Effect::Search): "[Player] searches their library for
/// [count] [filter] cards, [puts them somewhere], then shuffles" (CR 701.23a). The cards
/// found are revealed where the effect says so
/// ([CR 701.23e](https://yawgatog.com/resources/magic-rules/#R70123e)): "If the effect that
/// contains the search instruction doesn't also contain instructions to reveal the found
/// card(s), then they're not revealed." A player who finds nothing shuffles all the same
/// ([`shuffle::shuffle`]); one told to search for no cards does neither. The outcome is the
/// cards found.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    filter: Filter,
    count: Amount,
    found: Found,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    if count == 0 {
        return Ok(Outcome::NOTHING);
    }
    let mut all = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        let cards = search_library_up_to(game, resolution, player, filter, count)?;
        match found {
            // Candidate order: each card is revealed and moved alone, and with one card to
            // find, which is all a card of the pool searches for, there is no order.
            Found::RevealToHand => {
                for slot in game.state.in_candidate_order(cards) {
                    reveal::reveal(game, slot);
                    let card = game.state.object_ref(slot);
                    zones::move_to(game, card, Zone::Hand, Reason::Effect)?;
                }
            }
            Found::OntoBattlefieldTapped => zones::put_onto_battlefield_tapped(game, cards)?,
        }
        shuffle::shuffle(game, player);
        all |= cards;
    }
    Ok(Outcome::acted_on(all))
}
