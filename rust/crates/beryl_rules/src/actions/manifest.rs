//! Manifest ([CR 701.40](https://yawgatog.com/resources/magic-rules/#R70140)). Turning a
//! manifested permanent face up is a special action, in [`crate::face_down`].

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::Zone;

/// Manifests the card in `card` for `player`, from whatever zone it is in
/// ([CR 701.40a](https://yawgatog.com/resources/magic-rules/#R70140a)): "To manifest a card,
/// turn it face down. It becomes a 2/2 face-down creature card with no text, no name, no
/// subtypes, and no mana cost. Put that card onto the battlefield face down."
///
/// It is turned face down before it enters
/// ([CR 708.3](https://yawgatog.com/resources/magic-rules/#R7083)): "Objects that are put onto
/// the battlefield face down are turned face down before they enter the battlefield, so the
/// permanent's enters-the-battlefield abilities won't trigger (if triggered) or have any effect
/// (if static)." So it enters with [`FaceIx::DOWN`] up, and its own replacement and enters
/// abilities never apply.
///
/// The battlefield is a public zone, but a face-down card isn't seen
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Public zones are zones in
/// which all players can see the cards' faces, except for those cards that some rule or effect
/// specifically allow to be face down." Its controller may look at it (CR 708.5); another
/// player knows it only if they already did.
pub fn manifest<L: Layout>(
    game: &mut Game<'_, L>,
    card: L::Slot,
    player: Seat,
) -> Result<(), Full> {
    let s = &mut *game.state;
    let knew = s.who_knows(card);
    let entry = Entry {
        controller: Some(player),
        face: FaceIx::DOWN,
        ..Entry::default()
    };
    let (cards, to) = (Bitboard::single(card), Zone::Battlefield);
    event::move_batch(s, game.table, game.sink, cards, to, Reason::Effect, entry)?;
    // Each forgets alone: the result is the same in any order.
    let others = s.players().filter(|p| !knew.contains(p) && p != player);
    for p in s.players_in_seat_order(others) {
        s.forget(p, card);
    }
    Ok(())
}
