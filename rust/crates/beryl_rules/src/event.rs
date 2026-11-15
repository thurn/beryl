//! Events and the pipeline every game action goes through (architecture §8.1, §8.2,
//! decision D10).
//!
//! A primitive performs its mutation through here: zone changes get replacement effects,
//! look-back trigger matching with last-known information, execution, a layer refresh, then
//! trigger matching and the sink; every other event is executed by its primitive and then
//! [`settle`]d the same way. The replacement effects are applied by [`replace`].

pub use beryl_kernel::cause::{Cause, StackIx, Tag};
pub use beryl_kernel::event::{
    Event, EventKind, Listens, LossCause, Reason, Recipient, ReplacementKind, Replaces, Sink,
    Tagged, Work,
};

pub use crate::tokens::TokenEntry;
use beryl_kernel::mutate::Arrival;

use crate::bitboard::Bitboard;
use crate::card::{CastingKinds, FaceIx};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, Queue, Small};
use crate::replace;
use crate::state::{CounterKind, Memo, ObjectRef, State, Tracker, Zone};
use crate::table::GameTable;
use crate::trigger;
use crate::zone::Place;

/// How a permanent enters, as decided by whatever puts it onto the battlefield, before the
/// replacement effects of §8.2 apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Entry<L: Layout = Small> {
    pub tapped: bool,
    /// Put onto the battlefield attacking this player
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)).
    pub attacking: Option<Seat>,
    /// Under this player's control rather than its owner's.
    pub controller: Option<Seat>,
    /// The face up.
    pub face: FaceIx,
    /// Counters it enters with ("enters with N time counters").
    pub counters: Option<(CounterKind, u8)>,
    /// Its controller chose to pay the life an "unless you pay N life" replacement asks.
    pub pay_life: bool,
    /// What its spell was cast with that its abilities read, which it enters remembering
    /// ([`Memo`]): the kinds of cost paid, such as a kicker cost, and the value of X
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)).
    pub paid: CastingKinds,
    pub x: u8,
    /// The object an Aura spell attaches to as it resolves (CR 608.3c).
    pub attach: Option<L::Slot>,
}

/// Gives the permanent that just entered in `slot` what it enters with, has it remember
/// what its spell was cast with, and notes that it entered this turn, in a game with a card
/// that reads that.
pub(crate) fn arrive<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<(), Full> {
    s.track(t, Tracker::EnteredThisTurn, slot);
    s.arrive(
        slot,
        Arrival {
            tapped: entry.tapped,
            attacking: entry.attacking,
            controller: entry.controller,
            face: entry.face,
            counters: entry.counters,
            attach: entry.attach,
        },
    )?;
    if !entry.paid.is_empty() {
        s.remember(slot, Memo::Paid(entry.paid))?;
    }
    // Only a nonzero X is remembered: a permanent with no memo of one answers 0.
    if entry.x > 0 {
        s.remember(slot, Memo::X(entry.x))?;
    }
    Ok(())
}

/// Has each permanent that chose a value as it entered remember it ([`Memo::Chosen`]), once
/// the batch has moved and before the layers or any trigger look at it, and empties the list
/// for the next batch.
#[cold]
#[inline(never)]
fn remember_chosen<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>) -> Result<(), Full> {
    let chosen = std::mem::take(&mut sink.chosen);
    for &(slot, value) in chosen.iter() {
        s.remember(slot, Memo::Chosen(value))?;
    }
    Ok(())
}

/// Executes the remaining steps of the pipeline for an event its primitive has executed:
/// a layer refresh so nothing reads `derived` dirty, trigger matching (§8.2, step 5), and
/// the sink.
pub(crate) fn settle<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    e: Event<L>,
) -> Result<(), Full> {
    layers::refresh(s, t, sink)?;
    trigger::match_event(s, t, &e)?;
    sink.push(e);
    Ok(())
}

/// Moves one object.
pub(crate) fn move_one<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    to: Zone,
    reason: Reason,
) -> Result<(), Full> {
    move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        to,
        reason,
        Entry::default(),
    )
}

/// Moves a set of objects to one zone simultaneously, through the whole pipeline (§8.2).
///
/// 1. Each move is proposed ([`replace::Move`]), and replacement effects decide the object's
///    destination and how a permanent enters ([`replace::moving`]). The events are those of
///    the moves as replaced, so a creature exiled instead of being put into a graveyard did
///    not die.
/// 2. Leaves-the-battlefield abilities look back in time
///    ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): they are matched
///    against the objects as they were, before any of the batch moves, with last-known
///    information captured for their bodies.
/// 3. The moves execute.
/// 4. Layers are recomputed, then enters abilities are matched against the objects as they
///    are on the battlefield, and discard abilities against each card discarded
///    ([CR 603.6a](https://yawgatog.com/resources/magic-rules/#R6036a)): "Each time an event
///    puts one or more permanents onto the battlefield, all permanents on the battlefield
///    (including the newcomers) are checked for any enters-the-battlefield triggers that
///    match the event."
/// 5. Objects exiled "until" a permanent in the batch left the battlefield return, as one
///    more simultaneous batch ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)):
///    "A second one-shot effect is created immediately after the specified event. This
///    second one-shot effect returns the object to its previous zone."
///
/// The events of all of it are kept as one group ([`Sink::push_joined`]): the batch is one
/// event, and no player can act between it and the return the rules make "immediately
/// after" it, which is told from it by its [`Reason::Return`].
pub(crate) fn move_batch<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    move_batch_to(s, t, sink, objects, to, Place::Top, reason, entry)
}

/// [`move_batch`] to a place in a library other than the top.
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_batch_to<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    objects: Bitboard<L::Slot>,
    to: Zone,
    place: Place,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    layers::refresh(s, t, sink)?;
    // Each move as replaced: the object, where it was, where it goes and how it enters. A
    // tuple rather than a `replace::Move`: the queue is filled with defaults first, and
    // filling it with `replace::Move`s cost the rollout benchmark 1% more instructions.
    let mut moves: L::Queue<(L::Slot, Zone, Zone, Entry<L>), 128> = Default::default();
    let mut returning = Bitboard::EMPTY;
    for &slot in s.in_game_order(t, objects)?.as_slice() {
        let o = *s.object(slot);
        // CR 111.8: "A token that has left the battlefield can't move to another zone or
        // come back onto the battlefield. If such a token would change zones, it remains in
        // its current zone instead."
        if o.is_token() && !matches!(o.zone(), Zone::Battlefield | Zone::Nowhere) {
            continue;
        }
        let from = o.zone();
        let proposal = replace::Move {
            slot,
            from,
            to,
            entry,
            cast: reason == Reason::Resolve,
        };
        let m = replace::moving(s, t, sink, proposal)?;
        moves.push((m.slot, m.from, m.to, m.entry))?;
        if o.zone() == Zone::Battlefield {
            returning |= s.exiled_until(slot);
        }
    }
    // The permanents of the batch, each with where it goes. Read from the moves rather than
    // kept in a queue of their own, which every zone move would fill even in a game where
    // nothing looks back.
    let leaving = moves
        .iter()
        .filter(|&&(_, from, _, _)| from == Zone::Battlefield)
        .map(|&(slot, _, dest, _)| (slot, dest));
    trigger::match_look_back(s, t, leaving)?;
    let changed = |object, from, to| Event::ZoneChange {
        object,
        from,
        to,
        reason,
        // [CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Public zones
        // are zones in which all players can see the cards' faces" including the stack.
        // Keep the end chosen for a spell, even when the library was empty.
        library_top: match (from, to, place) {
            (Zone::Stack, Zone::Library, Place::Top) => Some(true),
            (Zone::Stack, Zone::Library, Place::Bottom) => Some(false),
            _ => None,
        },
    };
    let first = moves.as_slice().as_ptr();
    // The permanents the batch puts onto the battlefield, in the order they move.
    let mut arrived: L::Queue<L::Slot, 128> = Default::default();
    for each in moves.iter() {
        let &(slot, from, dest, entry) = each;
        if dest == Zone::Battlefield && entry.pay_life {
            replace::pay_life_to_enter(s, t, sink, slot, entry)?;
        }
        s.move_object(sink, slot, dest, place)?;
        if dest == Zone::Battlefield {
            arrive(s, t, slot, entry)?;
            arrived.push_bounded(slot);
        }
        sink.push_joined(changed(s.object_ref(slot), from, dest), || {
            !std::ptr::eq(each, first)
        });
    }
    if !sink.chosen.is_empty() {
        remember_chosen(s, sink)?;
    }
    layers::refresh(s, t, sink)?;
    if arrived.len() > 1 {
        crate::entered::order(s, t, arrived.as_slice())?;
        layers::refresh(s, t, sink)?;
    }
    for &(slot, from, dest, _) in moves.iter() {
        trigger::match_event(s, t, &changed(s.object_ref(slot), from, dest))?;
    }
    if !returning.is_empty() {
        // CR 610.3c: "An object returned to the battlefield this way returns under its
        // owner's control unless otherwise specified."
        sink.together_with_last();
        let returned = move_batch(
            s,
            t,
            sink,
            returning,
            Zone::Battlefield,
            Reason::Return,
            Entry::default(),
        );
        sink.apart();
        returned?;
    }
    Ok(())
}

/// Taps or untaps a permanent.
///
/// [CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a): "To tap a permanent,
/// turn it sideways from an upright position. Only untapped permanents can be tapped."
///
/// [CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "To untap a permanent,
/// rotate it back to the upright position from a sideways position. Only tapped permanents
/// can be untapped."
pub(crate) fn set_tapped<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    object: ObjectRef<L>,
    tapped: bool,
) -> Result<(), Full> {
    if !s.is_current(object) || s.object(object.slot).is_tapped() == tapped {
        return Ok(());
    }
    let e = s.set_tapped(object, tapped);
    settle(s, t, sink, e)
}
