//! Library ([CR 401](https://yawgatog.com/resources/magic-rules/#R401)).
//!
//! Implements: CR 401

use beryl_card::{
    Amount, LibraryOrder, LibraryPosition, LibraryRest, Objects, Players, Taken, prompt,
};

use crate::actions::reveal;
use crate::bitboard::Bitboard;
use crate::card::{Part, Prompt, Purpose, Stop};
use crate::casting;
use crate::filter::Evaluate;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::choices;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::replacement::event::{self, Entry, Event, Reason};
use crate::resolution::Resolution;
use crate::state::{ObjectRef, State, Zone, ZoneKey};
use crate::zone::Place;
use crate::zones;
use beryl_kernel::reference::Library;

/// The top `n` cards of a player's library, or all of it if it has fewer.
/// Uses the same card order as a draw ([`State::library_cards_from_top`]).
pub fn top<L: Layout>(s: &State<L>, player: Seat, n: u8) -> Bitboard<L::Slot> {
    s.library_cards_from_top(player)
        .take(usize::from(n))
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
/// ([`crate::replacement::exiled_instead`]). A card already in the library moves within it and
/// stays the object it was, so whoever knew it still does.
pub fn put_into<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    object: ObjectRef<L>,
    place: Place,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    match game.state.object(object.slot).zone() {
        Zone::Library => {
            if let Some(card) = game.state.card_in(object, Library) {
                game.state.reorder_library(card, place)?;
            }
            return Ok(());
        }
        Zone::Stack => {
            let Some(spell) = game.state.spell(object) else {
                return Ok(());
            };
            match casting::unstack(game.state, game.table, game.sink, spell, Zone::Library)? {
                Some(Zone::Library) => {}
                Some(to) => return zones::move_to(game, object, to, Reason::Effect),
                None => return Ok(()),
            }
        }
        Zone::Hand
        | Zone::Battlefield
        | Zone::Graveyard
        | Zone::Exile
        | Zone::Nowhere
        | Zone::Merged => {}
    }
    let (cards, to) = (Bitboard::single(object.slot), Zone::Library);
    let (reason, entry) = (Reason::Effect, Entry::default());
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_to_by(s, t, sink, player, cards, to, place, reason, entry)
}

/// Puts cards on the bottom of their owners' libraries in a random order. Their identities
/// are dealt anew among the slots they land in, so no player knows which is where
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "That library's owner
/// doesn't reveal the order in which the cards go into the library." The order comes from
/// the game's stream for everything but shuffles ([`crate::chance`]).
pub fn put_on_bottom_in_random_order<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let (to, place) = (Zone::Library, Place::Bottom);
    let (reason, entry) = (Reason::Effect, Entry::default());
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_to_by(s, t, sink, player, cards, to, place, reason, entry)?;
    // Rebound within each owner's library, among the slots that went there. Each takes
    // from the one random stream, by seat. A shared library is rebound once.
    for owner in s.zone_owners(to).in_seat_order() {
        let mut moved: L::Queue<L::Slot, 64> = Default::default();
        for &slot in s.library(owner) {
            if cards.contains(slot) {
                moved.push(slot)?;
            }
        }
        s.rebind_at_random(moved.as_slice())?;
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
    move_hand_cards_in_order(game, put, order.as_slice(), Place::Top)?;
    Ok(put)
}

/// The first part of a split takes as many cards as the player chooses
/// (see [CR 608.2d]): "the player announces these while applying the effect."
/// All chosen cards move together, each going below the one picked before it
/// ([CR 401.4]): "the owner of those cards may arrange them in any order."
///
/// [CR 608.2d]: https://yawgatog.com/resources/magic-rules/#R6082d
/// [CR 401.4]: https://yawgatog.com/resources/magic-rules/#R4014
fn put_any_on_bottom<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
) -> Result<Bitboard<L::Slot>, Stop> {
    let mut left = game.state.zone(ZoneKey::Of(player, Zone::Hand));
    let prompt = Prompt::new(Purpose::PutBack)
        .places(prompt::Place::Bottom, prompt::Place::None)
        .part(Part::First);
    let mut put = Bitboard::EMPTY;
    let mut order: L::Queue<L::Slot, 64> = Default::default();
    while !left.is_empty() {
        let Some(slot) = choices::pick_or_end(game, resolution, player, left, prompt)? else {
            break;
        };
        left.remove(slot);
        put.insert(slot);
        order.push(slot)?;
    }
    if !put.is_empty() {
        move_hand_cards_in_order(game, put, order.as_slice(), Place::Bottom)?;
    }
    Ok(put)
}

/// Moves the cards in one event (see [`event::move_batch`]). The batch lands in game
/// order, then each arrived card goes above or below the ones picked before it
/// ([CR 401.4]): "That library's owner doesn't reveal the order in which the cards go into
/// the library."
///
/// [CR 401.4]: https://yawgatog.com/resources/magic-rules/#R4014
fn move_hand_cards_in_order<L: Layout>(
    game: &mut Game<'_, L>,
    cards: Bitboard<L::Slot>,
    order: &[L::Slot],
    place: Place,
) -> Result<(), Full> {
    event::move_batch_to(
        game.state,
        game.table,
        game.sink,
        cards,
        Zone::Library,
        place,
        Reason::Effect,
        Entry::default(),
    )?;
    let state = &mut *game.state;
    for &slot in order {
        if let Some(card) = state.card_in(state.object_ref(slot), Library) {
            state.reorder_library(card, place)?;
        }
    }
    Ok(())
}

/// `player` looks at the top `n` cards of their library, or all of it if it has fewer, takes
/// some into their hand as `take` says, puts `top_count` of them on top of their library in
/// any order, and puts the rest where `rest` names. Looking is an exception to
/// [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't look at or
/// change the order of cards in a library." With fewer cards, the player follows the
/// instructions in order for the cards that are there (see [CR 609.3]). Returns the cards
/// taken into hand.
///
/// What is asked, which saves and rollouts replay ([`choices`]):
///
/// - [`Taken::None`]: no hand picks.
/// - [`Taken::Put`] of `k`: with `k` or fewer cards looked at, they all go to the hand
///   unasked. Otherwise one pick for each; when `top_count == 0`, at the first part of an
///   arrangement whose count is the cards looked at, otherwise as a [`Purpose::Look`] pick.
///   The cards go to the hand together.
/// - [`Taken::MayReveal`]: a look whose count is `n` as it was told, opening with whether to
///   take a card and then which, among those the filter accepts for the player
///   ([`choices::may_pick`]). The card taken is revealed
///   (see [CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
///   show that card to all players for a brief time."
/// - `top_count > 0`: one pick for each card placed on top (from the top down), then all
///   chosen cards move to the top in that order ([CR 401.4]).
/// - [`LibraryRest::Bottom`] with [`LibraryOrder::Any`]: one pick for each card of the rest at
///   the second part of the arrangement, whose count is the cards still to place, each going
///   under the ones before ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)):
///   "the owner of those cards may arrange them in any order."
/// - [`LibraryRest::Bottom`] with [`LibraryOrder::Random`]: nothing
///   ([`put_on_bottom_in_random_order`]).
/// - [`LibraryRest::Top`]: one pick for each card of the rest at the second part of an
///   arrangement, from the top down ([CR 401.4]).
///
/// [CR 609.3]: https://yawgatog.com/resources/magic-rules/#R6093
pub fn dig<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    n: u8,
    take: Taken,
    top_count: u8,
    rest: LibraryRest,
) -> Result<Bitboard<L::Slot>, Stop> {
    let looked = top(game.state, player, n);
    zones::look_at(game, player, looked);
    let rest_place = match rest {
        LibraryRest::Bottom(_) => prompt::Place::Bottom,
        LibraryRest::Top => prompt::Place::Top,
    };
    let arrange = Prompt::new(Purpose::Arrange).places(prompt::Place::Hand, rest_place);
    let cards = |cards: Bitboard<L::Slot>| u8::try_from(cards.count()).unwrap_or(u8::MAX);
    let mut taken = Bitboard::EMPTY;
    match take {
        Taken::None => {}
        Taken::Put(k) => {
            if cards(looked) <= k {
                taken = looked;
            } else if top_count == 0 {
                for left in (1..=k).rev() {
                    let hand = arrange.count(cards(looked)).most(left).part(Part::First);
                    let slot = choices::pick(game, resolution, player, looked & !taken, hand)?;
                    taken.insert(slot);
                }
            } else {
                for left in (1..=k).rev() {
                    let hand = Prompt::new(Purpose::Look)
                        .count(cards(looked))
                        .most(left)
                        .places(prompt::Place::Hand, prompt::Place::None);
                    let slot = choices::pick(game, resolution, player, looked & !taken, hand)?;
                    taken.insert(slot);
                }
            }
            zones::move_all_by(game, player, taken, Zone::Hand, Reason::Effect)?;
        }
        Taken::MayReveal(filter) => {
            let words = resolution.words(game);
            let accepted = filter.select(game.state, game.table, looked, player, &words);
            let look = Prompt::new(Purpose::Look)
                .count(n)
                .most(1)
                .places(prompt::Place::Hand, prompt::Place::None);
            if let Some(slot) = choices::may_pick(game, resolution, player, accepted, look)? {
                reveal::reveal(game, player, slot);
                let card = game.state.object_ref(slot);
                zones::move_to_by(game, player, card, Zone::Hand, Reason::Effect)?;
                taken.insert(slot);
            }
        }
    }
    let mut left = looked & !taken;
    if top_count > 0 && !left.is_empty() {
        if cards(left) <= top_count {
            let top_only =
                Prompt::new(Purpose::Arrange).places(prompt::Place::Top, prompt::Place::Top);
            order_on_top(game, resolution, player, &mut left, top_only, None)?;
        } else {
            let top_split = Prompt::new(Purpose::Arrange).places(prompt::Place::Top, rest_place);
            order_on_top(
                game,
                resolution,
                player,
                &mut left,
                top_split,
                Some(top_count),
            )?;
        }
    }
    match rest {
        LibraryRest::Bottom(LibraryOrder::Any) => {
            let bottom_arrange = if top_count > 0 {
                Prompt::new(Purpose::Arrange).places(prompt::Place::Top, prompt::Place::Bottom)
            } else {
                arrange
            };
            while !left.is_empty() {
                let bottom = bottom_arrange.count(cards(left)).part(Part::Second);
                let slot = choices::pick(game, resolution, player, left, bottom)?;
                put_into(game, player, game.state.object_ref(slot), Place::Bottom)?;
                left.remove(slot);
            }
        }
        LibraryRest::Bottom(LibraryOrder::Random) => {
            if !left.is_empty() {
                put_on_bottom_in_random_order(game, player, left)?;
            }
        }
        LibraryRest::Top => {
            let top_arrange = if matches!(take, Taken::None) {
                Prompt::new(Purpose::Arrange).places(prompt::Place::Top, prompt::Place::Top)
            } else {
                arrange
            };
            order_on_top(game, resolution, player, &mut left, top_arrange, None)?;
        }
    }
    Ok(taken)
}

/// Picks `count` cards (or all of `left` when `count` is `None`) from `left` from the top
/// down and puts them on top of their owner's library in that order ([CR 401.4]).
fn order_on_top<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    left: &mut Bitboard<L::Slot>,
    prompt: Prompt,
    count: Option<u8>,
) -> Result<(), Stop> {
    let cards = |cards: Bitboard<L::Slot>| u8::try_from(cards.count()).unwrap_or(u8::MAX);
    let mut order: L::Queue<L::Slot, 64> = Default::default();
    match count {
        Some(n) => {
            for rem in (1..=n).rev() {
                let pick = prompt.count(cards(*left)).most(rem).part(Part::First);
                let slot = choices::pick(game, resolution, player, *left, pick)?;
                left.remove(slot);
                order.push(slot)?;
            }
        }
        None => {
            while !left.is_empty() {
                let pick = prompt.count(cards(*left)).part(Part::Second);
                let slot = choices::pick(game, resolution, player, *left, pick)?;
                left.remove(slot);
                order.push(slot)?;
            }
        }
    }
    for &slot in order.iter().rev() {
        put_into(game, player, game.state.object_ref(slot), Place::Top)?;
    }
    Ok(())
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
        LibraryPosition::BottomRandom => {
            let cards = run.elsewhere_than(objects, Zone::Library);
            if !cards.is_empty() {
                run.remember(cards)?;
                let taker = run.taker();
                let game = run.game();
                let rest = crate::stack::unstack(game, cards, Zone::Library)?;
                put_on_bottom_in_random_order(game, taker, rest)?;
            }
            return Ok(run.now_in(cards, Zone::Library));
        }
        // CR 401.7: with "fewer than N cards in it, the player puts that card on the bottom
        // of that library."
        LibraryPosition::FromTop(n) => Place::UnderTop(n - 1),
        LibraryPosition::TopOrBottom => return put_at_owners_choice(run, objects),
    };
    Ok(zones::put_instruction(run, objects, Zone::Library, place)?)
}

/// [`LibraryPosition::TopOrBottom`]: "Its owner puts it on their choice of the top or bottom
/// of their library." Each card's owner is asked as the instruction is reached
/// (see [CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)), a yes for the top, and
/// the card goes there before the next is asked about. A card in a shared graveyard has no
/// owner, and the player who performs the instruction is asked instead. The outcome is the
/// cards that are in a library.
fn put_at_owners_choice<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Stop> {
    let objects = run.scene().objects(objects);
    run.remember(objects)?;
    let top_or_bottom =
        Prompt::new(Purpose::TopOrBottom).places(prompt::Place::Top, prompt::Place::Bottom);
    let taker = run.taker();
    for &slot in run.in_game_order(objects)?.iter() {
        let (game, resolution) = run.resolving();
        let object = game.state.object_ref(slot);
        let owner = if game.state.has_owner(slot) {
            game.state.object(slot).owner()
        } else {
            taker
        };
        let about = Bitboard::single(slot);
        let top = choices::yes_no(game, resolution, owner, top_or_bottom, about)?;
        put_into(
            game,
            owner,
            object,
            if top { Place::Top } else { Place::Bottom },
        )?;
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

/// [`Effect::PutAnyOnBottom`](beryl_card::Effect::PutAnyOnBottom): the outcome counts
/// the cards that reached the library, including zero when the player keeps their hand.
#[cold]
pub(crate) fn put_any_on_bottom_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
) -> Result<Outcome<L>, Stop> {
    let mut outcome = Outcome::NOTHING;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        let put = put_any_on_bottom(game, resolution, player)?;
        outcome = outcome.with(run.now_in(put, Zone::Library));
    }
    Ok(outcome)
}

/// [`Effect::Dig`](beryl_card::Effect::Dig): "[Player] looks at the top [look] cards of
/// their library", takes some into their hand, puts `top` on top, and puts the rest where
/// `rest` names (CR 401.2, CR 401.4). The outcome is the cards taken.
pub(crate) fn dig_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    look: Amount,
    take: Taken,
    top: u8,
    rest: LibraryRest,
) -> Result<Outcome<L>, Stop> {
    let look = run.scene().amount(look);
    let mut taken = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        taken |= dig(game, resolution, player, look, take, top, rest)?;
    }
    Ok(Outcome::acted_on(taken))
}
