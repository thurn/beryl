//! Search ([CR 701.23](https://yawgatog.com/resources/magic-rules/#R70123)).
//!
//! Implements: CR 701.23

use beryl_card::{Amount, Found, Players};

use crate::actions::{reveal, shuffle};
use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::casting::start::cast_during_resolution;
use crate::decision::Opt;
use crate::filter::{Evaluate, Filter};
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::choices;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::Reason;
use crate::resolution::Resolution;
use crate::state::{Zone, ZoneKey};
use crate::table::Capability;
use crate::zones;

/// `player` searches `owner`'s library for `n` cards that pass `filter`
/// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)): "To search for a card
/// in a zone, look at all cards in that zone (even if it's a hidden zone) and find a card that
/// matches the given description." When `filter` states a quality (anything other than
/// [`Filter::Any`]), the player may find fewer or none
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)): "If a player is
/// searching a hidden zone for cards with a stated quality, such as a card with a certain card
/// type or color, that player isn't required to find some or all of those cards even if they're
/// present in that zone." Otherwise the player must find as many cards as the library holds up
/// to `n`, and [`Opt::Done`](crate::decision::Opt::Done) is not offered while matching cards
/// remain. Returns the cards found, still in the library: the caller says where they go,
/// whether they are revealed
/// ([CR 701.23e](https://yawgatog.com/resources/magic-rules/#R70123e)), and shuffles.
#[inline]
pub fn search_library<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    owner: Seat,
    filter: Filter,
    n: u8,
) -> Result<Bitboard<L::Slot>, Stop> {
    let prompt = Prompt::new(Purpose::Search).most(n);
    let min = if filter == Filter::Any { n } else { 0 };
    // The prompt names what the search finds, as the card says it.
    resolution.described_by(filter, |resolution| {
        if game.table.can(Capability::CastWhileSearching) && player == owner && n != 0 {
            return search_with_casts(game, resolution, player, filter, min, n, prompt);
        }
        let library = game.state.zone(ZoneKey::Of(owner, Zone::Library));
        zones::look_at(game, player, library);
        let words = resolution.words(game);
        let found = filter.select(game.state, game.table, library, player, &words);
        choices::pick_up_to(game, resolution, player, found, min, n, prompt)
    })
}

/// A card in the player's own library can permit a cast before any card has been found.
/// See [CR 608.2g]: "That spell becomes the topmost object on the stack, and the currently
/// resolving spell or ability continues to resolve". The cast or its payment can change
/// the library, so each question uses the cards that are still there.
///
/// [CR 608.2g]: https://yawgatog.com/resources/magic-rules/#R6082g
#[cold]
#[inline(never)]
#[allow(
    clippy::disallowed_methods,
    reason = "reads each face the player may cast while searching"
)]
fn search_with_casts<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    filter: Filter,
    min: u8,
    n: u8,
    prompt: Prompt,
) -> Result<Bitboard<L::Slot>, Stop> {
    loop {
        let library = game.state.zone(ZoneKey::Of(player, Zone::Library));
        zones::look_at(game, player, library);
        let words = resolution.words(game);
        let found = filter.select(game.state, game.table, library, player, &words);
        let must_find = min > 0 && !found.is_empty();
        let pick =
            choices::search_pick(game, resolution, player, found, library, must_find, prompt)?;
        if let Opt::Cast { slot, face, .. } = pick {
            if cfg!(debug_assertions) {
                let def = game
                    .state
                    .card_def(game.table, slot)
                    .expect("a card offered to cast");
                for ability in def.face(face).abilities {
                    if ability.permits_cast_while_searching() {
                        crate::coverage::hit(ability);
                    }
                }
            }
            cast_during_resolution(game, resolution, player, pick, false)?;
            continue;
        }
        let Opt::Object(slot) = pick else {
            debug_assert_eq!(pick, Opt::Done);
            return Ok(Bitboard::EMPTY);
        };
        let first = Bitboard::single(slot);
        let rest = choices::pick_up_to(
            game,
            resolution,
            player,
            found & !first,
            min.saturating_sub(1),
            n - 1,
            prompt,
        )?;
        return Ok(first | rest);
    }
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
        let cards = search_library(game, resolution, player, player, filter, count)?;
        match found {
            Found::ToHand => {
                for &slot in game.state.in_game_order(game.table, cards)?.iter() {
                    let card = game.state.object_ref(slot);
                    zones::move_to_by(game, player, card, Zone::Hand, Reason::Effect)?;
                }
            }
            Found::RevealToHand => {
                for &slot in game.state.in_game_order(game.table, cards)?.iter() {
                    reveal::reveal(game, player, slot);
                    let card = game.state.object_ref(slot);
                    zones::move_to_by(game, player, card, Zone::Hand, Reason::Effect)?;
                }
            }
            Found::OntoBattlefieldTapped => {
                zones::put_onto_battlefield_tapped(game, player, cards)?;
            }
        }
        shuffle::shuffle(game, player)?;
        all |= cards;
    }
    Ok(Outcome::acted_on(all))
}
