//! Library ([CR 401](https://yawgatog.com/resources/magic-rules/#R401)).

use beryl_card::{LibraryPosition, Objects};

use crate::bitboard::Bitboard;
use crate::cast;
use crate::event::{self, Entry, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::state::{ObjectRef, State, Zone};
use crate::zone::Place;
use crate::zones;

/// The top `n` cards of a player's library, or all of it if it has fewer.
pub fn top<L: Layout>(s: &State<L>, player: Seat, n: u8) -> Bitboard<L::Slot> {
    let library = s.library(player);
    library[library.len().saturating_sub(usize::from(n))..]
        .iter()
        .copied()
        .collect()
}

/// Puts an object into its owner's library at `place`, from whatever zone it is in
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." A place under the top of a library with too few cards is its bottom
/// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes a
/// player to put a card into a library 'Nth from the top,' and that library has fewer than N
/// cards in it, the player puts that card on the bottom of that library."
///
/// A spell leaves the stack without resolving, or is exiled instead
/// ([`event::exiled_instead`]). A card already in the library moves within it and stays the
/// object it was, so whoever knew it still does.
pub fn put_into<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    place: Place,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    match game.state.object(object.slot).zone() {
        Zone::Library => return game.state.reorder_library(object.slot, place),
        Zone::Stack => match cast::unstack(game.state, object, Zone::Library) {
            Some(Zone::Library) => {}
            Some(to) => return zones::move_to(game, object, to, Reason::Effect),
            None => return Ok(()),
        },
        _ => {}
    }
    let (cards, to) = (Bitboard::single(object.slot), Zone::Library);
    let (reason, entry) = (Reason::Effect, Entry::default());
    event::move_batch_to(
        game.state, game.table, game.sink, cards, to, place, reason, entry,
    )
}

/// Puts cards on the bottom of their owners' libraries in a random order. Their identities
/// are dealt anew among the slots they land in, so no player knows which is where
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "That library's owner
/// doesn't reveal the order in which the cards go into the library." The order comes from
/// the game's stream for everything but shuffles ([`crate::chance`]).
pub fn put_on_bottom_in_random_order<L: Layout>(
    game: &mut Game<'_, L>,
    cards: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let (to, place) = (Zone::Library, Place::Bottom);
    let (reason, entry) = (Reason::Effect, Entry::default());
    event::move_batch_to(
        game.state, game.table, game.sink, cards, to, place, reason, entry,
    )?;
    // Rebound within each owner's library, among the slots that went there. Each takes
    // from the one random stream, by seat.
    let s = &mut *game.state;
    for owner in s.players_in_seat_order(s.players()) {
        let mut moved: L::Queue<L::Slot, 64> = Default::default();
        for &slot in s.library(owner) {
            if cards.contains(slot) {
                moved.push(slot)?;
            }
        }
        s.rebind_at_random(moved.as_slice());
    }
    Ok(())
}

/// [`Effect::PutIntoLibrary`](beryl_card::Effect::PutIntoLibrary): "Put [objects] into their
/// owners' libraries [position]" (CR 400.3). The outcome is the cards that are there.
pub(crate) fn put_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    position: LibraryPosition,
) -> Result<Outcome<L>, Full> {
    let place = match position {
        LibraryPosition::Top | LibraryPosition::FromTop(0 | 1) => Place::Top,
        LibraryPosition::Bottom => Place::Bottom,
        // CR 401.7: with "fewer than N cards in it, the player puts that card on the bottom
        // of that library."
        LibraryPosition::FromTop(n) => Place::UnderTop(n - 1),
    };
    zones::put_instruction(run, objects, Zone::Library, place)
}
