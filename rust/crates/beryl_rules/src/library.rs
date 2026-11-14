//! Library ([CR 401](https://yawgatog.com/resources/magic-rules/#R401)).

use beryl_card::{Amount, LibraryOrder, LibraryPosition, Objects, Players, Taken, prompt};

use crate::actions::reveal;
use crate::bitboard::Bitboard;
use crate::card::{Part, Prompt, Purpose, Stop};
use crate::cast;
use crate::choices;
use crate::event::{self, Entry, Event, Reason};
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::resolution::Resolution;
use crate::state::{ObjectRef, State, Zone, ZoneKey};
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
        Zone::Stack => match cast::unstack(game.state, game.sink, object, Zone::Library) {
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

/// `player` puts `n` cards from their hand on top of their library in any order, or their
/// whole hand if it has fewer
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two or
/// more cards in a specific position in a library at the same time, the owner of those cards
/// may arrange them in any order." One pick for each, each card going on top of the one
/// before, so the last picked is the top card: the first part of a split alone
/// ([`choices`]). The cards stay in the hand until the last is picked, then go to the
/// library at once, in the order picked. Returns the cards, in the library.
pub fn put_back<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    n: u8,
) -> Result<Bitboard<L::Slot>, Stop> {
    let hand = game.state.zone(ZoneKey::Of(player, Zone::Hand));
    let mut put = Bitboard::EMPTY;
    let mut order: L::Queue<L::Slot, 64> = Default::default();
    for left in (1..=n).rev() {
        if (hand & !put).is_empty() {
            break;
        }
        let put_back = Prompt::new(Purpose::PutBack)
            .most(left)
            .places(prompt::Place::OnTop, prompt::Place::None)
            .part(Part::First);
        let card = choices::pick(game, resolution, player, hand & !put, put_back)?;
        put.insert(card);
        order.push(card)?;
    }
    zones::move_all(game, put, Zone::Library, Reason::Effect)?;
    // The batch lands in game order; each card then goes on top of those picked before it.
    let s = &mut *game.state;
    for &slot in order.as_slice() {
        if s.object(slot).zone() == Zone::Library {
            s.reorder_library(slot, Place::Top)?;
        }
    }
    Ok(put)
}

/// `player` looks at the top `n` cards of their library, or all of it if it has fewer, takes
/// some into their hand as `take` says and puts the rest on the bottom in the order `rest`
/// names. Looking is an exception to
/// [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't look at or
/// change the order of cards in a library." Returns the cards taken.
///
/// What is asked, which saves and rollouts replay ([`choices`]):
///
/// - [`Taken::Put`] of `k`: with `k` or fewer cards looked at, they all go to the hand
///   unasked. Otherwise one pick for each at the first part of an arrangement, whose count
///   is the cards looked at; the cards go to the hand together.
/// - [`Taken::MayReveal`]: a look whose count is `n` as it was told, opening with whether to
///   take a card and then which, among those the filter accepts for the player
///   ([`choices::may_pick`]). The card taken is revealed
///   ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
///   show that card to all players for a brief time."
/// - [`LibraryOrder::Any`]: one pick for each card of the rest at the second part of the
///   arrangement, whose count is the cards still to place, each going under the ones before
///   ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "the owner of those
///   cards may arrange them in any order."
/// - [`LibraryOrder::Random`]: nothing ([`put_on_bottom_in_random_order`]).
pub fn dig<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    n: u8,
    take: Taken,
    rest: LibraryOrder,
) -> Result<Bitboard<L::Slot>, Stop> {
    let looked = top(game.state, player, n);
    zones::look_at(game, player, looked);
    // One prompt: the cards for the hand, then the order of the rest on the bottom.
    let arrange = Prompt::new(Purpose::Arrange).places(prompt::Place::Hand, prompt::Place::Bottom);
    let cards = |cards: Bitboard<L::Slot>| u8::try_from(cards.count()).unwrap_or(u8::MAX);
    let mut taken = Bitboard::EMPTY;
    match take {
        Taken::Put(k) => {
            if cards(looked) <= k {
                taken = looked;
            } else {
                for left in (1..=k).rev() {
                    let hand = arrange.count(cards(looked)).most(left).part(Part::First);
                    let slot = choices::pick(game, resolution, player, looked & !taken, hand)?;
                    taken.insert(slot);
                }
            }
            zones::move_all(game, taken, Zone::Hand, Reason::Effect)?;
        }
        Taken::MayReveal(filter) => {
            let accepted = filter.select(game.state, game.table, looked, player);
            let look = Prompt::new(Purpose::Look)
                .count(n)
                .most(1)
                .places(prompt::Place::Hand, prompt::Place::None);
            if let Some(slot) = choices::may_pick(game, resolution, player, accepted, look)? {
                reveal::reveal(game, slot);
                let card = game.state.object_ref(slot);
                zones::move_to(game, card, Zone::Hand, Reason::Effect)?;
                taken.insert(slot);
            }
        }
    }
    let mut left = looked & !taken;
    match rest {
        LibraryOrder::Any => {
            while !left.is_empty() {
                let bottom = arrange.count(cards(left)).part(Part::Second);
                let slot = choices::pick(game, resolution, player, left, bottom)?;
                put_into(game, game.state.object_ref(slot), Place::Bottom)?;
                left.remove(slot);
            }
        }
        LibraryOrder::Random => put_on_bottom_in_random_order(game, left)?,
    }
    Ok(taken)
}

/// [`Effect::PutIntoLibrary`](beryl_card::Effect::PutIntoLibrary): "Put [objects] into their
/// owners' libraries [position]" (CR 400.3). The outcome is the cards that are there.
pub(crate) fn put_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    position: LibraryPosition,
) -> Result<Outcome<L>, Stop> {
    let place = match position {
        LibraryPosition::Top | LibraryPosition::FromTop(0 | 1) => Place::Top,
        LibraryPosition::Bottom => Place::Bottom,
        // CR 401.7: with "fewer than N cards in it, the player puts that card on the bottom
        // of that library."
        LibraryPosition::FromTop(n) => Place::UnderTop(n - 1),
        LibraryPosition::TopOrBottom => return put_at_owners_choice(run, objects),
    };
    Ok(zones::put_instruction(run, objects, Zone::Library, place)?)
}

/// [`LibraryPosition::TopOrBottom`]: "Its owner puts it on their choice of the top or bottom
/// of their library." Each card's owner is asked as the instruction is reached
/// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)), a yes for the top, and
/// the card goes there before the next is asked about. The outcome is the cards that are in
/// a library.
fn put_at_owners_choice<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Stop> {
    let objects = run.scene().objects(objects);
    run.remember(objects)?;
    let top_or_bottom =
        Prompt::new(Purpose::TopOrBottom).places(prompt::Place::Top, prompt::Place::Bottom);
    // Candidate order: an order the game fixes, which is all a run of questions needs.
    for slot in run.state().in_candidate_order(objects) {
        let (game, resolution) = run.resolving();
        let (object, owner) = (game.state.object_ref(slot), game.state.object(slot).owner());
        let about = Bitboard::single(slot);
        let top = choices::yes_no(game, resolution, owner, top_or_bottom, about)?;
        put_into(game, object, if top { Place::Top } else { Place::Bottom })?;
        game.sink.note(Event::TopOrBottom { player: owner, top });
    }
    Ok(run.now_in(objects, Zone::Library))
}

/// [`Effect::PutBack`](beryl_card::Effect::PutBack): "[Player] puts [count] cards from their
/// hand on top of their library in any order" (CR 401.4). The outcome is the cards put
/// there.
pub(crate) fn put_back_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    count: Amount,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    let mut put = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        put |= put_back(game, resolution, player, count)?;
    }
    Ok(Outcome::acted_on(put))
}

/// [`Effect::Dig`](beryl_card::Effect::Dig): "[Player] looks at the top [look] cards of
/// their library", takes some into their hand and puts the rest on the bottom (CR 401.2,
/// CR 401.4). The outcome is the cards taken.
pub(crate) fn dig_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    look: Amount,
    take: Taken,
    rest: LibraryOrder,
) -> Result<Outcome<L>, Stop> {
    let look = run.scene().amount(look);
    let mut taken = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        taken |= dig(game, resolution, player, look, take, rest)?;
    }
    Ok(Outcome::acted_on(taken))
}
