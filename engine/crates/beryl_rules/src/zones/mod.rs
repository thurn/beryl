//! Zones (see [CR 400](https://yawgatog.com/resources/magic-rules/#R400)): moving an object from
//! one to another, and looking at the cards of a hidden one.
//!
//! Implements: CR 110.2a, CR 400.2, CR 400.3, CR 400.4a

pub mod counters;
pub mod drawing;
pub mod emblems;
pub mod library;
pub mod outside;
pub(crate) mod piles;
pub mod tokens;

use beryl_card::{Objects, Players, PutOnto, Types};
use beryl_kernel::reference::{Cards, ZoneMark};

use crate::bitboard::Bitboard;
use crate::card::FaceIx;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::objects::linked;
use crate::replacement::event::{self, Entry, Reason};
use crate::stack;
use crate::state::{CounterKind, ObjectRef, Zone};
use crate::zone::Place;
use beryl_kernel::memos::LinkKind;

/// Moves an object to a zone, to the top of an ordered one. One that has changed zones since
/// does not move (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object
/// that moves from one zone to another becomes a new object with no memory of, or relation
/// to, its previous existence."
#[inline]
pub fn move_to<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    event::move_one(game.state, game.table, game.sink, object.slot, to, reason)
}

/// [`move_to`] by `player`, who takes the object if it leaves a shared library or graveyard
/// and owns it from then on ([`Sink::taker`](event::Sink::taker)).
// Always inline: the move of every game of another format runs through here, and
// the test of the format must cost it no call.
#[inline(always)]
#[allow(clippy::inline_always)]
pub fn move_to_by<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    object: ObjectRef<L>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_one_by(s, t, sink, player, object.slot, to, reason)
}

/// Moves a set of objects to one zone simultaneously, each to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone."
#[inline]
pub fn move_all<L: Layout>(
    game: &mut Game<'_, L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    let entry = Entry::default();
    event::move_batch(
        game.state, game.table, game.sink, objects, to, reason, entry,
    )
}

/// [`move_all`] by `player`, who takes the objects that leave a shared library or graveyard and
/// owns them from then on ([`Sink::taker`](event::Sink::taker)).
// Always inline: the move of every game of another format runs through here, and
// the test of the format must cost it no call.
#[inline(always)]
#[allow(clippy::inline_always)]
pub fn move_all_by<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    objects: Bitboard<L::Slot>,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_by(s, t, sink, player, objects, to, reason, Entry::default())
}

/// `payer` exiles cards chosen for a cost together, and takes those that leave a shared
/// graveyard. A token does not represent a card
/// (see [CR 111.6]) and cannot enter the payment's checked set.
///
/// ```compile_fail,E0308
/// use beryl_kernel::reference::{Graveyard, Permanents};
/// use beryl_rules::{Game, zones::exile_cards_for_cost};
///
/// fn pay(game: &mut Game<'_>, tokens: Permanents) {
///     let payer = beryl_kernel::ids::Seat::P0;
///     let _ = exile_cards_for_cost::<Graveyard, _>(game, payer, tokens.slots());
/// }
/// ```
///
/// [CR 111.6]: https://yawgatog.com/resources/magic-rules/#R1116
#[inline]
pub fn exile_cards_for_cost<Z: ZoneMark, L: Layout>(
    game: &mut Game<'_, L>,
    payer: Seat,
    cards: Cards<Z, L>,
) -> Result<(), Full> {
    move_all_by(game, payer, cards.slots(), Zone::Exile, Reason::Exile)
}

/// Puts a permanent card onto the battlefield under `controller`'s control, with the face up
/// that it enters with and any counters it enters with
/// ([CR 110.2a](https://yawgatog.com/resources/magic-rules/#R1102a)): "If an effect instructs
/// a player to put an object onto the battlefield, that object enters the battlefield under
/// that player's control unless the effect states otherwise."
pub fn put_onto_battlefield<L: Layout>(
    game: &mut Game<'_, L>,
    object: ObjectRef<L>,
    controller: Seat,
    face: FaceIx,
    counters: Option<(CounterKind, u8)>,
) -> Result<(), Full> {
    if !game.state.is_current(object) {
        return Ok(());
    }
    let entry = Entry {
        controller: Some(controller),
        face,
        counters,
        ..Entry::default()
    };
    let (objects, to) = (Bitboard::single(object.slot), Zone::Battlefield);
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    // A card that leaves a shared library or graveyard is taken by the player it enters under.
    event::move_batch_by(s, t, sink, controller, objects, to, Reason::Effect, entry)
}

/// [`Effect::ReturnToBattlefield`](beryl_card::Effect::ReturnToBattlefield): "Return
/// [cards] to the battlefield under their owners' control", from whatever zone each is in,
/// all at once, untapped and as new objects
/// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)). The outcome is the
/// permanents they are.
///
/// An instant or sorcery card stays where it is
/// ([CR 400.4a](https://yawgatog.com/resources/magic-rules/#R4004a)): "If an instant or
/// sorcery card would enter the battlefield, it remains in its previous zone." A token that
/// left the battlefield has ceased to exist
/// (see [CR 111.7](https://yawgatog.com/resources/magic-rules/#R1117)), so nothing names it.
pub(crate) fn return_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    onto_battlefield(run, cards, Entry::default())
}

/// [`Effect::PutOntoBattlefield`](beryl_card::Effect::PutOntoBattlefield): "Return [objects]
/// to the battlefield under your control", from whatever zone each is in, all at once and as
/// new objects (see CR 400.7), under the control of the one player [`PutOnto::under`] names or
/// else of each one's owner
/// ([CR 110.2a](https://yawgatog.com/resources/magic-rules/#R1102a)): "If an effect
/// instructs a player to put an object onto the battlefield, that object enters the
/// battlefield under that player's control unless the effect states otherwise." An instant
/// or sorcery card stays where it is (CR 400.4a). The outcome is the permanents they are.
///
/// A linked instruction has each permanent remember the ability's source
/// ([`LinkKind::PutOntoBattlefieldWith`]), while that source is still the object whose ability
/// this is: Animate Dead's "enchant creature put onto the battlefield with this Aura" and
/// "that creature" read it.
pub(crate) fn put_onto_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    onto: &PutOnto,
) -> Result<Outcome<L>, Full> {
    // A permanent has one controller: players that are not one player put nothing there,
    // which the card table refuses to say (`Malformed::Under`).
    let controller = match onto.under {
        Some(players) => match run.scene().players(players).sole() {
            Some(player) => Some(player),
            None => return Ok(Outcome::NOTHING),
        },
        None => None,
    };
    let source = run.resolution().source();
    let entry = Entry {
        controller,
        tapped: onto.tapped,
        ..Entry::default()
    };
    let outcome = onto_battlefield(run, onto.objects, entry)?;
    if onto.linked {
        let link = LinkKind::PutOntoBattlefieldWith;
        linked::link(run.game(), outcome.objects, source, link)?;
    }
    Ok(outcome)
}

/// Puts the objects `objects` names onto the battlefield from whatever zone each is in, all at
/// once, as `entry` says, but for instant and sorcery cards, which stay where they are
/// (CR 400.4a). The outcome is the permanents they are.
fn onto_battlefield<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    entry: Entry<L>,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, Zone::Battlefield);
    // "Under its owner's control" of a card in a shared zone, which has no owner, is under the
    // control of the player who performs the instruction.
    let you = run.you();
    let game = run.game();
    let objects = objects.filter(|slot| {
        // A split card off the stack has both halves' card types (see CR 709.4c).
        let types = game.state.copiable(slot).types(game.table);
        !types.intersects(Types::INSTANT.union(Types::SORCERY))
    });
    let (to, reason) = (Zone::Battlefield, Reason::Effect);
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_by(s, t, sink, you, objects, to, reason, entry)?;
    Ok(run.now_in(objects, Zone::Battlefield))
}

/// `player` puts cards onto the battlefield tapped, each under its owner's control,
/// simultaneously: "Those cards enter tapped." A card that leaves a shared library or graveyard
/// has no owner, and enters under `player`'s control and owned by them.
pub fn put_onto_battlefield_tapped<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    cards: Bitboard<L::Slot>,
) -> Result<(), Full> {
    let entry = Entry {
        tapped: true,
        ..Entry::default()
    };
    let (to, reason) = (Zone::Battlefield, Reason::Effect);
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_by(s, t, sink, player, cards, to, reason, entry)
}

/// `player` looks at the cards in `cards` and from then on knows them (architecture §13): a
/// library and a hand are hidden zones
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "zones in which not all
/// players can be expected to see the cards' faces."
#[inline]
pub fn look_at<L: Layout>(game: &mut Game<'_, L>, player: Seat, cards: Bitboard<L::Slot>) {
    game.state.look(game.sink, player, cards);
}

/// [`Effect::LookAt`](beryl_card::Effect::LookAt): "[Player] looks at [cards]" (CR 400.2):
/// [`look_at`], for each player. The outcome is the cards.
pub(crate) fn look_at_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    cards: Objects,
) -> Outcome<L> {
    let cards = run.scene().objects(cards);
    for player in run.in_turn_order(players) {
        look_at(run.game(), player, cards);
    }
    Outcome::acted_on(cards)
}

/// [`Effect::ReturnToHand`](beryl_card::Effect::ReturnToHand): "Return [objects] to their
/// owners' hands." The outcome is the cards that are there.
pub(crate) fn return_to_hand_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    put_instruction(run, objects, Zone::Hand, Place::Top)
}

/// [`Effect::PutIntoGraveyard`](beryl_card::Effect::PutIntoGraveyard): "Put [objects] into
/// their owners' graveyards" (CR 400.3), each in the order the game lists them. The outcome
/// is the cards it moved, wherever a replacement effect put them: what the effect goes on
/// to say of "it" is said of the card, not of where it went.
pub(crate) fn put_into_graveyard_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, Zone::Graveyard);
    if objects.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(objects)?;
    // A spell among them leaves the stack without resolving, and one that is exiled
    // instead has moved already.
    let rest = stack::unstack(run.game(), objects, Zone::Graveyard)?;
    let mut moved = objects & !rest;
    let you = run.you();
    for &slot in run.in_game_order(rest)?.iter() {
        let object = run.state().object_ref(slot);
        let pile = run.with_piles(Bitboard::single(slot));
        move_to_by(run.game(), you, object, Zone::Graveyard, Reason::Effect)?;
        if !run.state().is_current(object) {
            moved |= pile;
        }
    }
    Ok(Outcome::acted_on(moved))
}

/// Puts the objects `objects` names into their owners' hands or libraries, at `place` in a
/// library (CR 400.3): the arm of [`return_to_hand_instruction`] and of
/// [`library::put_instruction`](crate::zones::library::put_instruction). A spell among them leaves
/// the stack without resolving. The outcome is the cards that are there.
pub(crate) fn put_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    to: Zone,
    place: Place,
) -> Result<Outcome<L>, Full> {
    let objects = run.elsewhere_than(objects, to);
    if objects.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    run.remember(objects)?;
    let found = run.with_piles(objects);
    // "Its owner's hand" of a card in a shared zone is the hand of the player who performs the
    // instruction, who takes it.
    let you = run.you();
    let game = run.game();
    let rest = stack::unstack(game, objects, to)?;
    let (reason, entry) = (Reason::Effect, Entry::default());
    let (s, t, sink) = (&mut *game.state, game.table, &mut *game.sink);
    event::move_batch_to_by(s, t, sink, you, rest, to, place, reason, entry)?;
    Ok(run.now_in(found, to))
}
