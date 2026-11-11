//! Events and the pipeline every game action goes through (architecture §8.1, §8.2,
//! decision D10).
//!
//! A primitive performs its mutation through here: zone changes get replacement effects,
//! look-back trigger matching with last-known information, execution, a layer refresh, then
//! trigger matching and the sink; every other event is executed by its primitive and then
//! [`settle`]d the same way. The cube's other replacement and prevention effects (damage,
//! most draws) arrive with the cards that create them.

pub use beryl_kernel::event::{Event, LossCause, Reason, Recipient, Sink, Work, kind};
use beryl_kernel::mutate::Arrival;

use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, AbilityKind, FaceAbility, FaceIx, Replacement, Types};
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layers;
use crate::layout::{Layout, Queue, Small};
use crate::query::Copiable;
use crate::state::{Continuous, CounterKind, Counters, ObjectFlags, ObjectRef, State, Zone};
use crate::table::GameTable;
use crate::trigger;
use crate::zone::Place;

/// How a permanent enters, as decided by whatever puts it onto the battlefield, before the
/// replacement effects of §8.2 apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Entry<L: Layout = Small> {
    pub tapped: bool,
    /// Put onto the battlefield attacking
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)).
    pub attacking: bool,
    /// Under this player's control rather than its owner's.
    pub controller: Option<Seat>,
    /// The face up.
    pub face: FaceIx,
    /// Counters it enters with ("enters with N time counters").
    pub counters: Option<(CounterKind, u8)>,
    /// Its controller chose to pay the life an "unless you pay N life" replacement asks.
    pub pay_life: bool,
    /// What its spell was cast with that its abilities read: [`ObjectFlags`] such as
    /// kicked, and the value of X
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)).
    pub flags: u32,
    pub x: u8,
    /// The object an Aura spell attaches to as it resolves (CR 608.3c).
    pub attach: Option<L::Slot>,
}

/// Gives the permanent that just entered in `slot` what it enters with. The kinds of counter
/// and the flags milestone 4 (by-w8gv.7.1) moves to memos go raw, through the legacy accessor.
fn arrive<L: Layout>(s: &mut State<L>, slot: L::Slot, entry: Entry<L>) {
    let long_tail = entry
        .counters
        .filter(|&(kind, _)| Counters::is_long_tail(kind));
    s.arrive(
        slot,
        Arrival {
            tapped: entry.tapped,
            attacking: entry.attacking,
            controller: entry.controller,
            face: entry.face,
            counters: if long_tail.is_some() {
                None
            } else {
                entry.counters
            },
            attach: entry.attach,
        },
    );
    if let Some((kind, n)) = long_tail {
        s.legacy_mut().set_counters(slot, kind, n);
    }
    s.legacy_mut().set_object_flags(slot, entry.flags, true);
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
    layers::refresh(s, t, sink);
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
/// 1. Replacement effects decide each object's destination and how a permanent enters.
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
    layers::refresh(s, t, sink);
    let mut moves: L::Queue<(L::Slot, Zone, Zone, Entry<L>), 128> = Default::default();
    let mut returning = Bitboard::EMPTY;
    for &slot in s.in_game_order(t, objects)?.as_slice() {
        let o = *s.object(slot);
        // CR 111.8: "A token that has left the battlefield can't move to another zone or
        // come back onto the battlefield. If such a token would change zones, it remains in
        // its current zone instead."
        if o.flags().has(ObjectFlags::TOKEN)
            && !matches!(o.zone(), Zone::Battlefield | Zone::Nowhere)
        {
            continue;
        }
        let dest = destination(s, slot, to);
        let entry = if dest == Zone::Battlefield {
            enter_replacements(s, t, slot, entry)
        } else {
            entry
        };
        moves.push((slot, o.zone(), dest, entry))?;
        if o.zone() == Zone::Battlefield {
            trigger::match_leaving(s, t, slot, dest)?;
            returning |= exiled_until(s, slot);
        }
    }
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
    for &(slot, from, dest, entry) in moves.iter() {
        if dest == Zone::Battlefield && entry.pay_life {
            pay_life_to_enter(s, t, sink, slot, entry)?;
        }
        s.move_object(sink, slot, dest, place)?;
        if dest == Zone::Battlefield {
            arrive(s, slot, entry);
            if entry.x > 0 {
                let r = s.object_ref(slot);
                s.legacy_mut().remember_x(r, entry.x)?;
            }
        }
        sink.push(changed(s.object_ref(slot), from, dest));
    }
    layers::refresh(s, t, sink);
    for &(slot, from, dest, _) in moves.iter() {
        trigger::match_event(s, t, &changed(s.object_ref(slot), from, dest))?;
    }
    if !returning.is_empty() {
        // CR 610.3c: "An object returned to the battlefield this way returns under its
        // owner's control unless otherwise specified."
        move_batch(
            s,
            t,
            sink,
            returning,
            Zone::Battlefield,
            Reason::Return,
            Entry::default(),
        )?;
    }
    Ok(())
}

/// Where an object actually goes after floating replacement effects. The only one the cards
/// create is "exile it instead" of a graveyard, and no two of them can apply to one event
/// differently, so the ordering of [CR 616.1](https://yawgatog.com/resources/magic-rules/#R6161)
/// never becomes a choice.
fn destination<L: Layout>(s: &State<L>, slot: L::Slot, to: Zone) -> Zone {
    let r = s.object_ref(slot);
    let exiled = to == Zone::Graveyard
        && s.effects()
            .iter()
            .any(|e| e.affected == r && e.effect == Continuous::ExileInsteadOfGraveyard);
    if exiled { Zone::Exile } else { to }
}

/// The objects exiled until the permanent in `slot` leaves the battlefield.
fn exiled_until<L: Layout>(s: &State<L>, slot: L::Slot) -> Bitboard<L::Slot> {
    s.in_zone(Zone::Exile).filter(|x| {
        let o = s.object(x);
        o.flags().has(ObjectFlags::EXILED_UNTIL) && o.linked() == Some(slot)
    })
}

/// Applies the permanent's own replacement effects that modify how it enters (CR 614.12):
/// they "check the characteristics of the permanent as it would exist on the battlefield".
/// They are self-replacement effects of a kind that never conflict, so each applies in
/// turn. Then the planeswalker's intrinsic one
/// ([CR 306.5b](https://yawgatog.com/resources/magic-rules/#R3065b)): "A planeswalker has the
/// intrinsic ability 'This permanent enters with a number of loyalty counters on it equal to
/// its printed loyalty number.'"
fn enter_replacements<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    mut entry: Entry<L>,
) -> Entry<L> {
    let o = s.object(slot);
    let face = s.copiable(slot).turned(entry.face).face(t);
    let controller = entry.controller.unwrap_or(o.owner());
    for ability in face.abilities {
        if ability.kind != AbilityKind::Replacement {
            continue;
        }
        match ability.replacement {
            // Not about entering: the draw machinery reads it.
            Replacement::None | Replacement::WinInsteadOfDrawingFromEmptyLibrary => continue,
            Replacement::EntersTapped => entry.tapped = true,
            Replacement::EntersTappedUnlessFewLands(n) => {
                // Not yet on the battlefield, so every land counted is another land.
                let lands = s
                    .battlefield()
                    .iter()
                    .filter(|&&p| {
                        let d = s.derived(p);
                        d.controller == controller && d.types.contains(Types::LAND)
                    })
                    .count();
                if lands > usize::from(n) {
                    entry.tapped = true;
                }
            }
            Replacement::EntersTappedUnlessPayLife(_) => {
                if !entry.pay_life {
                    entry.tapped = true;
                }
            }
        }
        crate::coverage::hit(ability);
    }
    if let Some(loyalty) = face.loyalty {
        entry.counters = Some((CounterKind::Loyalty, loyalty));
    }
    entry
}

/// Pays the life an "unless you pay N life" entering replacement asks for
/// ([CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)): "If a player pays life,
/// the payment is subtracted from their life total; in other words, the player loses that
/// much life."
fn pay_life_to_enter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<(), Full> {
    let o = s.object(slot);
    let face = s.copiable(slot).turned(entry.face).face(t);
    let player = entry.controller.unwrap_or(o.owner());
    for ability in face.abilities {
        if let Replacement::EntersTappedUnlessPayLife(n) = ability.replacement {
            lose_life(s, t, sink, player, n)?;
        }
    }
    Ok(())
}

/// The life an entering replacement of the face asks for, if any.
pub(crate) fn life_to_enter(t: &GameTable, entering: Copiable) -> Option<u8> {
    entering
        .face(t)
        .abilities
        .iter()
        .find_map(|a| match a.replacement {
            Replacement::EntersTappedUnlessPayLife(n) => Some(n),
            _ => None,
        })
}

/// A player loses life ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "If an
/// effect causes a player to gain life or lose life, that player's life total is adjusted
/// accordingly."
pub(crate) fn lose_life<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    amount: u8,
) -> Result<(), Full> {
    let e = s.lose_life(player, amount);
    settle(s, t, sink, e)
}

/// A player gains life (CR 119.3).
pub(crate) fn gain_life<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    amount: u8,
) -> Result<(), Full> {
    let e = s.gain_life(player, amount);
    settle(s, t, sink, e)
}

/// A player draws cards, one at a time
/// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a player is instructed
/// to draw multiple cards, that player performs that many individual card draws." The event
/// counts the cards actually drawn, if any.
///
/// Each draw from an empty library is offered to Jace, Wielder of Mysteries' replacement
/// first ([CR 614.11](https://yawgatog.com/resources/magic-rules/#R61411)): "Some effects
/// replace card draws. These effects are applied even if no cards could be drawn because
/// there are no cards in the affected player's library." Winning ends the game at once
/// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
/// that a player wins the game." The draws left are never made.
pub(crate) fn draw<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    cards: u8,
) -> Result<(), Full> {
    let mut drawn = 0;
    let mut wins = None;
    for _ in 0..cards {
        if let Some(&slot) = s.library(player).last() {
            drawn += 1;
            sink.note(Event::DrawCard { slot });
        } else {
            wins = wins_instead_of_drawing(s, t, player);
            if wins.is_some() {
                break;
            }
        }
        s.draw(sink, player)?;
    }
    if drawn > 0 {
        settle(
            s,
            t,
            sink,
            Event::Draw {
                player,
                cards: drawn,
            },
        )?;
    }
    if let Some(cause) = wins {
        crate::sba::win_game(s, sink, player, cause);
    }
    Ok(())
}

/// Why `player`'s opponent loses, if `player` controls a permanent whose replacement effect
/// has them win rather than draw from an empty library: that ability.
fn wins_instead_of_drawing<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Option<LossCause<L>> {
    s.battlefield()
        .iter()
        .filter(|&&slot| s.controller(slot) == player)
        .find_map(|&slot| {
            let (index, a) = trigger::face(s, t, slot)
                .abilities
                .iter()
                .enumerate()
                .find(|(_, a)| a.replacement == Replacement::WinInsteadOfDrawingFromEmptyLibrary)?;
            crate::coverage::hit(a);
            let (card, face) = s.copiable(slot).record(t);
            Some(LossCause::Effect {
                source: Some(s.object_ref(slot)),
                card,
                face,
                ability: Some(FaceAbility::Own(AbilityIx::new(
                    u8::try_from(index).expect("a face has fewer than 256 abilities"),
                ))),
            })
        })
}

/// Deals damage ([CR 120.3](https://yawgatog.com/resources/magic-rules/#R1203)).
///
/// [CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a): "Damage dealt to a player
/// by a source without infect causes that player to lose that much life."
///
/// [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
/// planeswalker causes that many loyalty counters to be removed from that planeswalker."
///
/// [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a
/// creature by a source with neither wither nor infect causes that much damage to be marked on
/// that creature."
pub(crate) fn deal_damage<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    source: ObjectRef<L>,
    to: Recipient<L>,
    amount: u8,
    combat: bool,
) -> Result<(), Full> {
    if amount == 0 {
        return Ok(());
    }
    let e = match to {
        Recipient::Player(p) => s.damage_player(source, p, amount, combat),
        Recipient::Permanent(r) => {
            if !s.is_current(r) || s.object(r.slot).zone() != Zone::Battlefield {
                return Ok(());
            }
            s.mark_damage(source, r, amount, combat)
        }
    };
    settle(s, t, sink, e)
}

/// Puts counters on a permanent
/// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)).
pub(crate) fn add_counters<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    object: ObjectRef<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !s.is_current(object) || amount == 0 {
        return Ok(());
    }
    let layered = layers::counters_matter(s, t, object.slot, kind);
    let e = if Counters::is_long_tail(kind) {
        s.legacy_mut().add_counters(object, kind, amount, layered)
    } else {
        s.add_counters(object, kind, amount, layered)
    };
    settle(s, t, sink, e)
}

/// Removes up to `amount` counters of a kind from a permanent.
pub(crate) fn remove_counters<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    object: ObjectRef<L>,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !s.is_current(object) {
        return Ok(());
    }
    // Read before writing: asking for the count to change would make `kind` the object's
    // long-tail kind even when it has none to remove.
    let amount = amount.min(s.object(object.slot).counters_of(kind));
    if amount == 0 {
        return Ok(());
    }
    let layered = layers::counters_matter(s, t, object.slot, kind);
    let e = if Counters::is_long_tail(kind) {
        s.legacy_mut()
            .remove_counters(object, kind, amount, layered)
    } else {
        s.remove_counters(object, kind, amount, layered)
    };
    settle(s, t, sink, e)
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

/// How a token is created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenEntry {
    pub tapped: bool,
    /// Created attacking (CR 508.4): never declared, so no attack triggers.
    pub attacking: bool,
}

/// Creates `count` tokens with the copiable values `what` under `controller`'s control: a
/// predefined token's, or those of what the tokens copy.
///
/// [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "Some effects put tokens
/// onto the battlefield." [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The
/// player who creates a token is its owner."
/// [CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072): "the copy acquires the
/// copiable values of the original object's characteristics".
///
/// The tokens take free slots from the shared pool (architecture §4.2) and enter as one
/// event; then one `TokensCreated` event follows for "whenever you create one or more".
pub(crate) fn create_tokens<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    controller: Seat,
    what: Copiable,
    count: u8,
    how: TokenEntry,
) -> Result<Bitboard<L::Slot>, Full> {
    let mut tokens = Bitboard::EMPTY;
    let mut free = L::Slot::pool().filter(|&slot| s.object(slot).zone() == Zone::Nowhere);
    for _ in 0..count {
        let slot = free.next().ok_or(Full)?;
        tokens.insert(slot);
    }
    // A copy of a double-faced permanent enters with the same face up (CR 707.8a).
    let mut face = FaceIx::FRONT;
    // Each token is made in its own slot, with no event; they enter together below, in the
    // batch's order.
    for slot in s.in_candidate_order(tokens) {
        face = s.make_token(t, slot, controller, what);
    }
    move_batch(
        s,
        t,
        sink,
        tokens,
        Zone::Battlefield,
        Reason::Created,
        Entry {
            tapped: how.tapped,
            attacking: how.attacking,
            face,
            ..Entry::default()
        },
    )?;
    settle(s, t, sink, Event::TokensCreated { controller, tokens })?;
    Ok(tokens)
}
