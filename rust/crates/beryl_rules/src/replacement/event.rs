//! Events and the pipeline every game action goes through (architecture §8.1, §8.2,
//! decision D10).
//!
//! A primitive performs its mutation through here: zone changes get replacement effects,
//! look-back trigger matching with last-known information, execution, a layer refresh, then
//! trigger matching and the sink; every other event is executed by its primitive and then
//! [`settle`]d the same way. The replacement effects are applied by [`replacement`].
//!
//! Implements: none. Coordinates replacement, mutation, and trigger matching for events.

pub use beryl_kernel::cause::{Cause, StackIx, Tag};
pub use beryl_kernel::event::{
    Event, EventKind, Listens, LossCause, Reason, Recipient, ReplacementKind, Replaces, Sink,
    Tagged, Work,
};

pub use crate::zones::tokens::TokenEntry;
use beryl_kernel::mutate::Arrival;

use crate::bitboard::Bitboard;
use crate::card::{CastingKinds, FaceIx, Subtype};
use crate::continuous::layers;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::replacement;
use crate::stack::trigger;
use crate::state::{CounterKind, Memo, ObjectRef, State, Tracker, Zone};
use crate::table::{Capability, GameTable};
use crate::zone::Place;

/// How a permanent enters, as decided by whatever puts it onto the battlefield, before the
/// replacement effects of §8.2 apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Entry<L: Layout = Small> {
    pub tapped: bool,
    /// Put onto the battlefield attacking this player
    /// (see [CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)).
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
    /// (see [CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)).
    pub paid: CastingKinds,
    pub x: u8,
    /// The object an Aura spell attaches to as it resolves (see CR 608.3c).
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
    let permanent = s
        .permanent_at(slot)
        .expect("an object just moved onto the battlefield");
    s.track(t, Tracker::EnteredThisTurn, slot);
    s.arrive(
        permanent,
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

/// Starts the copy effect of the permanent that just arrived in `slot`, if it chose to enter as
/// a copy ([`Sink::copies`]), before the layers or any trigger look at it (see [CR 707.5]): "It
/// doesn't enter the battlefield, and then become a copy of that permanent. ... Also, any
/// enters-the-battlefield triggered abilities of the copy will have a chance to trigger."
///
/// [CR 707.5]: https://yawgatog.com/resources/magic-rules/#R7075
#[cold]
#[inline(never)]
fn start_copy<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, slot: L::Slot) -> Result<(), Full> {
    let Some(i) = sink.copies.iter().position(|&(copier, _)| copier == slot) else {
        return Ok(());
    };
    let (_, values) = sink.copies.remove(i);
    let permanent = s
        .permanent_at(slot)
        .expect("a copy entering the battlefield");
    s.enter_as_copy(permanent, values)
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
/// 1. Each move is proposed ([`replacement::Move`]), and replacement effects decide the object's
///    destination and how a permanent enters ([`replacement::moving`]). The events are those of
///    the moves as replaced, so a creature exiled instead of being put into a graveyard did
///    not die.
/// 2. Leaves-the-battlefield abilities look back in time
///    (see [CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): they are matched
///    against the objects as they were, before any of the batch moves, with last-known
///    information captured for their bodies.
/// 3. The moves execute.
/// 4. Layers are recomputed, then enters abilities are matched against the objects as they
///    are on the battlefield, and discard abilities against each card discarded
///    (see [CR 603.6a](https://yawgatog.com/resources/magic-rules/#R6036a)): "Each time an event
///    puts one or more permanents onto the battlefield, all permanents on the battlefield
///    (including the newcomers) are checked for any enters-the-battlefield triggers that
///    match the event."
/// 5. Objects exiled "until" a permanent in the batch left the battlefield return, as one
///    more simultaneous batch (see [CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)):
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
    // Moving one object needs neither sorting nor scratch space for a full batch.
    if let Some(slot) = objects.sole() {
        return move_ordered::<L, 1>(s, t, sink, &[slot], to, place, reason, entry);
    }
    let ordered = s.in_game_order(t, objects)?;
    move_ordered::<L, 128>(s, t, sink, ordered.as_slice(), to, place, reason, entry)
}

/// A prepared move: the slot, the old and new zones, and how it enters.
type PreparedMove<L> = (<L as Layout>::Slot, Zone, Zone, Entry<L>);

/// Runs the zone-change pipeline for `ordered`, in the steps [`move_batch`] describes.
#[allow(clippy::too_many_arguments)]
fn move_ordered<L: Layout, const N: usize>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    ordered: &[L::Slot],
    to: Zone,
    place: Place,
    reason: Reason,
    entry: Entry<L>,
) -> Result<(), Full> {
    // Each move as replaced: the object, where it was, where it goes and how it enters. A
    // tuple rather than a `replacement::Move`: the queue is filled with defaults first, and
    // filling it with `replacement::Move`s cost the rollout benchmark 1% more instructions.
    let mut moves: L::Queue<PreparedMove<L>, N> = Default::default();
    let mut returning = Bitboard::EMPTY;
    // Whether the game has an Aura, which may choose what it enchants as it enters and may be
    // attached to a card in a graveyard: a game with none asks neither below.
    let auras = t.subtypes().has(Subtype::Aura).possible();
    let mut prepare = |s: &mut State<L>, slot| {
        let proposal = replacement::Move {
            slot,
            from: s.object(slot).zone(),
            to,
            entry,
            cast: reason == Reason::Resolve,
        };
        let prepared = prepare_move(s, t, sink, proposal, auras)?;
        if prepared.is_some() && proposal.from == Zone::Battlefield {
            returning |= s.exiled_until(slot);
        }
        Ok(prepared)
    };
    if N > 1
        && to == Zone::Battlefield
        && entry.controller.is_none()
        && t.can(Capability::ChoosesAsEnters)
    {
        prepare_entries::<L, N>(s, ordered, &mut moves, prepare)?;
    } else {
        for &slot in ordered {
            if let Some(m) = prepare(s, slot)? {
                moves.push(m)?;
            }
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
        // see [CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Public zones
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
    let mut arrived: L::Queue<L::Slot, N> = Default::default();
    for each in moves.iter() {
        let &(slot, from, dest, entry) = each;
        if dest == Zone::Battlefield && entry.pay_life {
            replacement::pay_life_to_enter(s, t, sink, slot, entry)?;
        }
        if auras && from == Zone::Graveyard {
            unattach_from(s, slot);
        }
        s.move_object(sink, slot, dest, place)?;
        if dest == Zone::Battlefield {
            arrive(s, t, slot, entry)?;
            // See CR 702.37c: "When the spell resolves, it enters the battlefield with the
            // same characteristics the spell had." Its controller still pays its morph cost.
            if from == Zone::Stack && entry.face == FaceIx::DOWN {
                s.remember(slot, Memo::Morphed)?;
            }
            if !sink.copies.is_empty() {
                start_copy(s, sink, slot)?;
            }
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
        crate::continuous::arrival_timestamps::order(s, t, arrived.as_slice())?;
        layers::refresh(s, t, sink)?;
    }
    for &(slot, from, dest, _) in moves.iter() {
        trigger::match_event(s, t, &changed(s.object_ref(slot), from, dest))?;
    }
    if !returning.is_empty() {
        // see CR 610.3c: "An object returned to the battlefield this way returns under its
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

/// Applies the replacements and attachment choices for one move before the batch moves.
/// For an Aura that enters without a specified attachment, "that player chooses what it will
/// enchant as the Aura enters the battlefield" (see [CR 303.4f]). If nothing is legal to enchant,
/// "the Aura remains in its current zone, unless that zone is the stack" (see [CR 303.4g]).
///
/// [CR 303.4f]: https://yawgatog.com/resources/magic-rules/#R3034f
/// [CR 303.4g]: https://yawgatog.com/resources/magic-rules/#R3034g
#[inline]
fn prepare_move<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    proposal: replacement::Move<L>,
    auras: bool,
) -> Result<Option<PreparedMove<L>>, Full> {
    let replacement::Move { slot, from, .. } = proposal;
    // see CR 111.8: "A token that has left the battlefield can't move to another zone or
    // come back onto the battlefield. If such a token would change zones, it remains in
    // its current zone instead." A token on the stack is a copy of a spell, which leaves
    // it as a spell does: onto the battlefield as a token as it resolves
    // (see [CR 608.3f](https://yawgatog.com/resources/magic-rules/#R6083f)), or to where a
    // card would go, where the state-based check ends it (see CR 704.5e).
    if s.object(slot).is_token() && !matches!(from, Zone::Battlefield | Zone::Nowhere | Zone::Stack)
    {
        return Ok(None);
    }
    let mut m = replacement::moving(s, t, sink, proposal)?;
    if auras && m.to == Zone::Battlefield && m.entry.attach.is_none() {
        match replacement::enchants(s, t, sink, slot, m.entry)? {
            replacement::Enchants::NotAnAura => {}
            replacement::Enchants::Object(host) => m.entry.attach = Some(host),
            replacement::Enchants::Nothing => {
                sink.copies.retain(|&(copier, _)| copier != slot);
                if from != Zone::Stack {
                    s.end_exiled_until(slot);
                    return Ok(None);
                }
                m.to = Zone::Graveyard;
            }
        }
    }
    Ok(Some((m.slot, m.from, m.to, m.entry)))
}

/// When each permanent enters under its owner's control, prepare the moves by those players
/// in turn order (see [CR 101.4]): "the active player (the player whose turn it is) makes any
/// choices required, then the next player in turn order ... makes any choices required".
/// "Then the actions happen simultaneously." Keep the prepared moves in their original order
/// so committing the batch still reports the events in game order.
///
/// [CR 101.4]: https://yawgatog.com/resources/magic-rules/#R1014
#[cold]
#[inline(never)]
fn prepare_entries<L: Layout, const N: usize>(
    s: &mut State<L>,
    ordered: &[L::Slot],
    moves: &mut L::Queue<PreparedMove<L>, N>,
    mut prepare: impl FnMut(&mut State<L>, L::Slot) -> Result<Option<PreparedMove<L>>, Full>,
) -> Result<(), Full> {
    let mut prepared: L::Queue<Option<PreparedMove<L>>, N> = Default::default();
    for _ in ordered {
        prepared.push(None)?;
    }
    for player in s.players_in_turn_order(s.players()) {
        for (&slot, result) in ordered.iter().zip(prepared.as_mut_slice()) {
            if s.object(slot).owner() == player {
                *result = prepare(s, slot)?;
            }
        }
    }
    for &m in prepared.iter().flatten() {
        moves.push(m)?;
    }
    Ok(())
}

/// Unattaches each Aura attached to the card in `slot`, which is leaving a graveyard, as
/// Animate Dead is attached while its enters ability waits. The object the card becomes is
/// new (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), so the Aura is
/// attached to nothing, and goes to the graveyard as a state-based action (see CR 704.5m) unless
/// an instruction attaches it again.
#[cold]
#[inline(never)]
fn unattach_from<L: Layout>(s: &mut State<L>, slot: L::Slot) {
    for i in 0..s.battlefield().len() {
        let aura = s.battlefield()[i];
        if s.object(aura).attached_to() == Some(slot) {
            let aura = s.permanent_at(aura).expect("an Aura on the battlefield");
            s.unattach(aura);
        }
    }
}

/// Taps or untaps a permanent.
///
/// see [CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a): "To tap a permanent,
/// turn it sideways from an upright position. Only untapped permanents can be tapped."
///
/// see [CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "To untap a permanent,
/// rotate it back to the upright position from a sideways position. Only tapped permanents
/// can be untapped."
// Inline: every mana payment taps through here, and out of line it cost the rollout
// benchmark about a million instructions more than inlined into its callers.
#[inline]
pub(crate) fn set_tapped<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    object: ObjectRef<L>,
    tapped: bool,
) -> Result<(), Full> {
    let Some(permanent) = s.permanent(object) else {
        return Ok(());
    };
    if s.object(permanent.slot()).is_tapped() == tapped {
        return Ok(());
    }
    let e = s.set_tapped(permanent, tapped);
    settle(s, t, sink, e)
}
