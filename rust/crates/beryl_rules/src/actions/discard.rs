//! Discard ([CR 701.9](https://yawgatog.com/resources/magic-rules/#R7019)).

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::choices;
use crate::event::Reason;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{ObjectRef, Zone, ZoneKey};
use crate::zones;

/// Discards a card from its owner's hand
/// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
/// move it from its owner's hand to that player's graveyard." One no longer in a hand is not
/// discarded (CR 400.7).
#[inline]
pub fn discard<L: Layout>(game: &mut Game<'_, L>, card: ObjectRef<L>) -> Result<(), Full> {
    if !game.state.is_current(card) || game.state.object(card.slot).zone() != Zone::Hand {
        return Ok(());
    }
    zones::move_to(game, card, Zone::Graveyard, Reason::Discard)
}

/// `player` discards `n` cards of their choice, or their whole hand if it has fewer
/// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)): "By default, effects
/// that cause a player to discard a card allow the affected player to choose which card to
/// discard." One pick for each: the picks of one prompt, which says at each how many are
/// still to discard. Returns the cards, in the graveyard.
pub fn discard_cards<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    n: u8,
) -> Result<Bitboard<L::Slot>, Stop> {
    let mut discarded = Bitboard::EMPTY;
    for left in (1..=n).rev() {
        let hand = game.state.zone(ZoneKey::Of(player, Zone::Hand));
        let Some(card) = discard_of(game, resolution, player, hand, left)? else {
            break;
        };
        discarded.insert(card.slot);
    }
    Ok(discarded)
}

/// `player` discards one of `cards`, cards in their hand, of their choice (CR 701.9b), as
/// when told to discard a card of some kind. Returns the card in the graveyard, or `None`
/// when there is none to discard.
pub fn discard_one_of<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
) -> Result<Option<ObjectRef<L>>, Stop> {
    discard_of(game, resolution, player, cards, 1)
}

/// One discard of `left` still to make, among `cards`.
fn discard_of<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
    left: u8,
) -> Result<Option<ObjectRef<L>>, Stop> {
    debug_assert!((cards & !game.state.zone(ZoneKey::Of(player, Zone::Hand))).is_empty());
    if cards.is_empty() {
        return Ok(None);
    }
    let prompt = Prompt::new(Purpose::Discard).most(left);
    let slot = choices::pick(game, resolution, player, cards, prompt)?;
    discard(game, game.state.object_ref(slot))?;
    Ok(Some(game.state.object_ref(slot)))
}

/// [`Effect::Discard`](beryl_card::Effect::Discard): "[Player] discards [count] cards", each
/// of their choice (CR 701.9b), or their whole hand if it has fewer
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)). The outcome is the cards
/// discarded.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    let mut discarded = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        discarded |= discard_cards(game, resolution, player, count)?;
    }
    Ok(Outcome::acted_on(discarded))
}
