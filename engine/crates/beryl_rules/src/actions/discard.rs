//! Discard ([CR 701.9](https://yawgatog.com/resources/magic-rules/#R7019)).
//!
//! Implements: CR 701.9

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose, Stop};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::choices;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::event::{self, Entry, Reason};
use crate::resolution::Resolution;
use crate::state::{Zone, ZoneKey};
use crate::zones;
use beryl_kernel::reference::{CardIn, Hand};

/// Discards a card from its owner's hand
/// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
/// move it from its owner's hand to that player's graveyard." One that has left the hand since
/// is not discarded (see CR 400.7).
#[inline]
pub fn discard<L: Layout>(game: &mut Game<'_, L>, card: CardIn<Hand, L>) -> Result<(), Full> {
    // In a game whose players share a graveyard, a discarded card has no owner once it is
    // there, so the move names its discarder for the discard triggers to read.
    let discarder = game.state.object(card.slot()).owner();
    zones::move_to_by(
        game,
        discarder,
        card.into(),
        Zone::Graveyard,
        Reason::Discard,
    )
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
        discarded.insert(card);
    }
    Ok(discarded)
}

/// `player` discards one of `cards`, cards in their hand, of their choice (CR 701.9b), as
/// when told to discard a card of some kind. Returns the card discarded, or `None` when there
/// is none to discard.
pub fn discard_one_of<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
) -> Result<Option<L::Slot>, Stop> {
    discard_of(game, resolution, player, cards, 1)
}

/// One discard of `left` still to make, among `cards`.
fn discard_of<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
    left: u8,
) -> Result<Option<L::Slot>, Stop> {
    debug_assert!((cards & !game.state.zone(ZoneKey::Of(player, Zone::Hand))).is_empty());
    if cards.is_empty() {
        return Ok(None);
    }
    let prompt = Prompt::new(Purpose::Discard).most(left);
    let slot = choices::pick(game, resolution, player, cards, prompt)?;
    // A token that has left the battlefield can't move to another zone (see CR 111.8), so one
    // picked from a hand stays there.
    if let Some(card) = game.state.card_in(game.state.object_ref(slot), Hand) {
        discard(game, card)?;
    }
    Ok(Some(slot))
}

/// [`Effect::Discard`](beryl_card::Effect::Discard): "[Player] discards [count] cards", each
/// of their choice (CR 701.9b), or their whole hand if it has fewer
/// (see [CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)). The outcome is the cards
/// discarded.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Stop> {
    // Discarding the whole hand requires no choice. Move all the cards together
    // (see CR 701.9a), even in a layout that can hold more than 255 cards.
    if players == Players::You
        && matches!(count, Amount::Count(cards) if *cards == beryl_card::prelude::hand_of(&Players::You))
    {
        let cards = run
            .scene()
            .objects(beryl_card::prelude::hand_of(&Players::You));
        let you = run.you();
        let game = run.game();
        let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
        let (to, reason, entry) = (Zone::Graveyard, Reason::Discard, Entry::default());
        event::move_batch_by(s, t, sink, you, cards, to, reason, entry)?;
        return Ok(Outcome::acted_on(cards));
    }
    let count = run.scene().amount(count)?;
    let mut discarded = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        discarded |= discard_cards(game, resolution, player, count)?;
    }
    Ok(Outcome::acted_on(discarded))
}
