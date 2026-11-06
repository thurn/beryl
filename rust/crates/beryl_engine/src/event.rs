//! Events and the pipeline every game action goes through (architecture §8.1, §8.2,
//! decision D10).
//!
//! A primitive performs its mutation through here: zone changes get replacement effects,
//! look-back trigger matching with last-known information, execution, a layer refresh, then
//! trigger matching and the sink; every other event is executed by its primitive and then
//! [`settle`]d the same way. The cube's other replacement and prevention effects (damage,
//! most draws) arrive with the cards that create them.

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, FaceIx, Replacement, Types};
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot};
use crate::layers;
use crate::state::{
    Continuous, CounterKind, ObjectFlags, ObjectRef, State, TurnStep, Zone, token_slots,
};
use crate::table::GameTable;
use crate::trigger;
use crate::zone::{self, Place};

/// Why an object changed zones. Triggers and replacement effects read it ("dies", "is put
/// into a graveyard from anywhere", "is discarded").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// Played as a land (CR 305.1).
    Play,
    /// Drawn (CR 121.1).
    Draw,
    /// Discarded (CR 701.9a).
    Discard,
    /// Destroyed (CR 701.8a).
    Destroy,
    /// Sacrificed (CR 701.21a).
    Sacrifice,
    /// A state-based action (CR 704).
    StateBased,
    /// Exiled by an effect (CR 701.13a).
    Exile,
    /// A token created on the battlefield (CR 111.1).
    Created,
    /// Returned when an "until" exile ends (CR 610.3).
    Return,
    /// Any other effect that puts an object into a zone.
    Effect,
    /// Put on the stack as it is cast (CR 601.2a).
    Cast,
    /// Countered ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)).
    Counter,
    /// A spell resolving or failing to: a permanent spell entering the battlefield, an
    /// instant or sorcery put into its owner's graveyard (CR 608.2n, 608.3).
    Resolve,
}

/// The player or permanent dealt damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Recipient {
    Player(Seat),
    Permanent(ObjectRef),
}

/// One thing that happened (architecture §8.1). Simultaneous happenings that rules read as
/// one carry a set: one `TokensCreated` for every token an effect creates, one `Attack` for
/// every attacker declared. Flag writes and cache updates never go through events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// An object changed zones; `object` is the new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).
    ZoneChange {
        object: ObjectRef,
        from: Zone,
        to: Zone,
        reason: Reason,
    },
    Damage {
        source: ObjectRef,
        to: Recipient,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Seat,
        cards: u8,
    },
    LifeGain {
        player: Seat,
        amount: u8,
    },
    LifeLoss {
        player: Seat,
        amount: u8,
    },
    CountersAdded {
        object: ObjectRef,
        kind: CounterKind,
        amount: u8,
    },
    CountersRemoved {
        object: ObjectRef,
        kind: CounterKind,
        amount: u8,
    },
    Tap {
        object: ObjectRef,
    },
    Untap {
        object: ObjectRef,
    },
    /// "Whenever you create one or more tokens" reads this once per event.
    TokensCreated {
        controller: Seat,
        tokens: Bitboard,
    },
    /// A spell was cast ([CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i)).
    Cast {
        spell: ObjectRef,
        controller: Seat,
    },
    /// A player declared attackers ([CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)).
    Attack {
        player: Seat,
        attackers: Bitboard,
    },
    /// A step or phase began ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)).
    StepBegins {
        step: TurnStep,
        active: Seat,
    },
    GainControl {
        object: ObjectRef,
        player: Seat,
    },
    /// A coin flip ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the
    /// call matches the result, the player wins the flip."
    CoinFlip {
        player: Seat,
        won: bool,
    },
    Lose {
        player: Seat,
    },
    Win {
        player: Seat,
    },
}

/// One bit per kind of event a triggered ability can listen to, for the per-card masks of
/// the game table (§6.4): a kind no card listens to costs one AND.
pub(crate) mod kind {
    pub const ENTERS: u16 = 1 << 0;
    pub const ATTACK: u16 = 1 << 1;
    pub const DAMAGE: u16 = 1 << 2;
    pub const STEP: u16 = 1 << 3;
    pub const TOKENS: u16 = 1 << 4;
    pub const CAST: u16 = 1 << 5;
    pub const DIES: u16 = 1 << 6;
    pub const DISCARD: u16 = 1 << 7;
    pub const DRAW: u16 = 1 << 8;
}

impl Event {
    /// The listener kind of this event, or 0 if no trigger listens to it.
    pub(crate) fn kind(&self) -> u16 {
        match self {
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            } => kind::ENTERS,
            Event::ZoneChange {
                reason: Reason::Discard,
                ..
            } => kind::DISCARD,
            Event::Attack { .. } => kind::ATTACK,
            Event::Damage { .. } => kind::DAMAGE,
            Event::StepBegins { .. } => kind::STEP,
            Event::TokensCreated { .. } => kind::TOKENS,
            Event::Cast { .. } => kind::CAST,
            Event::Draw { .. } => kind::DRAW,
            _ => 0,
        }
    }
}

/// Where executed events go for the UI or a replay (§8.2, step 6). Search leaves it off.
#[derive(Debug, Default)]
pub struct Sink {
    events: Vec<Event>,
    on: bool,
    /// The work done since the scratch last took the counts: the sink goes everywhere the
    /// engine does it.
    pub(crate) work: Work,
}

/// Counts of the engine's work below `next` and `apply`, moved into the scratch's
/// [`crate::Counts`] when they return.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Work {
    /// Events executed, kept or not.
    pub(crate) events: u64,
    pub(crate) layer_recomputes: u64,
    pub(crate) solver_runs: u64,
}

impl Sink {
    /// A sink that drops every event.
    pub fn off() -> Self {
        Self::default()
    }

    /// A sink that keeps every event until drained.
    pub fn recording() -> Self {
        Self {
            on: true,
            ..Self::default()
        }
    }

    /// The events since the last drain, oldest first.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// How many events it holds.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drops the events after the first `len`, those of a body run discarded (§7.3).
    pub(crate) fn truncate(&mut self, len: usize) {
        self.events.truncate(len);
    }

    pub(crate) fn push(&mut self, e: Event) {
        self.work.events += 1;
        if self.on {
            self.events.push(e);
        }
    }
}

/// How a permanent enters, as decided by whatever puts it onto the battlefield, before the
/// replacement effects of §8.2 apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Entry {
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
    pub attach: Option<Slot>,
}

/// Executes the remaining steps of the pipeline for an event its primitive has executed:
/// a layer refresh so nothing reads `derived` dirty, trigger matching (§8.2, step 5), and
/// the sink.
pub(crate) fn settle(s: &mut State, t: &GameTable, sink: &mut Sink, e: Event) -> Result<(), Full> {
    layers::refresh(s, t, sink);
    trigger::match_event(s, t, &e)?;
    sink.push(e);
    Ok(())
}

/// Moves one object.
pub(crate) fn move_one(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: Slot,
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
pub(crate) fn move_batch(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    objects: Bitboard,
    to: Zone,
    reason: Reason,
    entry: Entry,
) -> Result<(), Full> {
    move_batch_to(s, t, sink, objects, to, Place::Top, reason, entry)
}

/// [`move_batch`] to a place in a library other than the top.
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_batch_to(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    objects: Bitboard,
    to: Zone,
    place: Place,
    reason: Reason,
    entry: Entry,
) -> Result<(), Full> {
    layers::refresh(s, t, sink);
    let mut moves: Fixed<(Slot, Zone, Zone, Entry), 128> = Fixed::new();
    let mut returning = Bitboard::EMPTY;
    for slot in objects.iter() {
        let o = *s.object(slot);
        // CR 111.8: "A token that has left the battlefield can't move to another zone or
        // come back onto the battlefield. If such a token would change zones, it remains in
        // its current zone instead."
        if o.flags.has(ObjectFlags::TOKEN) && !matches!(o.zone, Zone::Battlefield | Zone::Nowhere) {
            continue;
        }
        let dest = destination(s, slot, to);
        let entry = if dest == Zone::Battlefield {
            enter_replacements(s, t, slot, entry)
        } else {
            entry
        };
        moves.push((slot, o.zone, dest, entry))?;
        if o.zone == Zone::Battlefield {
            trigger::match_leaving(s, t, slot, dest)?;
            returning |= exiled_until(s, slot);
        }
    }
    for &(slot, from, dest, entry) in &moves {
        if dest == Zone::Battlefield && entry.pay_life {
            pay_life_to_enter(s, t, sink, slot, entry)?;
        }
        zone::move_to_place(s, slot, dest, place)?;
        if dest == Zone::Battlefield {
            let o = s.object_mut(slot);
            o.flags.set(ObjectFlags::TAPPED, entry.tapped);
            o.flags.set(ObjectFlags::ATTACKING, entry.attacking);
            o.face = entry.face;
            if let Some(controller) = entry.controller {
                o.base_controller = controller;
            }
            if let Some((kind, n)) = entry.counters {
                *o.counters.get_mut(kind) = n;
            }
            o.flags.0 |= entry.flags;
            if let Some(host) = entry.attach {
                o.attached_to = Some(host);
            }
            if entry.x > 0 {
                let r = s.object_ref(slot);
                let mut i = 0;
                while i < s.entered_x.len() {
                    if s.is_current(s.entered_x.as_slice()[i].0) {
                        i += 1;
                    } else {
                        s.entered_x.remove(i);
                    }
                }
                s.entered_x.push((r, entry.x))?;
            }
        }
        sink.push(Event::ZoneChange {
            object: s.object_ref(slot),
            from,
            to: dest,
            reason,
        });
    }
    layers::refresh(s, t, sink);
    for &(slot, from, dest, _) in &moves {
        let e = Event::ZoneChange {
            object: s.object_ref(slot),
            from,
            to: dest,
            reason,
        };
        trigger::match_event(s, t, &e)?;
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
fn destination(s: &State, slot: Slot, to: Zone) -> Zone {
    let r = s.object_ref(slot);
    let exiled = to == Zone::Graveyard
        && s.effects
            .iter()
            .any(|e| e.affected == r && e.effect == Continuous::ExileInsteadOfGraveyard);
    if exiled { Zone::Exile } else { to }
}

/// The objects exiled until the permanent in `slot` leaves the battlefield.
fn exiled_until(s: &State, slot: Slot) -> Bitboard {
    (s.zone(Seat::P0, Zone::Exile) | s.zone(Seat::P1, Zone::Exile))
        .iter()
        .filter(|&x| {
            let o = s.object(x);
            o.flags.has(ObjectFlags::EXILED_UNTIL) && o.linked == Some(slot)
        })
        .collect()
}

/// Applies the permanent's own replacement effects that modify how it enters (CR 614.12):
/// they "check the characteristics of the permanent as it would exist on the battlefield".
/// They are self-replacement effects of a kind that never conflict, so each applies in
/// turn. Then the planeswalker's intrinsic one
/// ([CR 306.5b](https://yawgatog.com/resources/magic-rules/#R3065b)): "A planeswalker has the
/// intrinsic ability 'This permanent enters with a number of loyalty counters on it equal to
/// its printed loyalty number.'"
fn enter_replacements(s: &State, t: &GameTable, slot: Slot, mut entry: Entry) -> Entry {
    let o = s.object(slot);
    let face = t.def(o.card).face(entry.face);
    let controller = entry.controller.unwrap_or(o.owner);
    for ability in face.abilities {
        if ability.kind != AbilityKind::Replacement {
            continue;
        }
        match ability.replacement {
            // Not about entering: the draw machinery reads it.
            Replacement::None | Replacement::WinInsteadOfDrawingFromEmptyLibrary => {}
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
fn pay_life_to_enter(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: Slot,
    entry: Entry,
) -> Result<(), Full> {
    let o = s.object(slot);
    let face = t.def(o.card).face(entry.face);
    let player = entry.controller.unwrap_or(o.owner);
    for ability in face.abilities {
        if let Replacement::EntersTappedUnlessPayLife(n) = ability.replacement {
            lose_life(s, t, sink, player, n)?;
        }
    }
    Ok(())
}

/// The life an entering replacement of the face asks for, if any.
pub(crate) fn life_to_enter(t: &GameTable, card: CardIx, face: FaceIx) -> Option<u8> {
    t.def(card)
        .face(face)
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
pub(crate) fn lose_life(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    amount: u8,
) -> Result<(), Full> {
    s.player_mut(player).life -= i16::from(amount);
    settle(s, t, sink, Event::LifeLoss { player, amount })
}

/// A player gains life (CR 119.3).
pub(crate) fn gain_life(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    amount: u8,
) -> Result<(), Full> {
    s.player_mut(player).life += i16::from(amount);
    settle(s, t, sink, Event::LifeGain { player, amount })
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
pub(crate) fn draw(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    cards: u8,
) -> Result<(), Full> {
    let mut drawn = 0;
    let mut wins = false;
    for _ in 0..cards {
        if s.library(player).is_empty() {
            if wins_instead_of_drawing(s, t, player) {
                wins = true;
                break;
            }
        } else {
            drawn += 1;
        }
        zone::draw(s, player)?;
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
    if wins {
        crate::sba::end_game(s, sink, player.opponent());
    }
    Ok(())
}

/// Whether `player` controls a permanent whose replacement effect has them win rather than
/// draw from an empty library.
fn wins_instead_of_drawing(s: &State, t: &GameTable, player: Seat) -> bool {
    s.battlefield().iter().any(|&slot| {
        s.controller(slot) == player
            && trigger::face(s, t, slot)
                .abilities
                .iter()
                .any(|a| a.replacement == Replacement::WinInsteadOfDrawingFromEmptyLibrary)
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
pub(crate) fn deal_damage(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    source: ObjectRef,
    to: Recipient,
    amount: u8,
    combat: bool,
) -> Result<(), Full> {
    if amount == 0 {
        return Ok(());
    }
    match to {
        Recipient::Player(p) => s.player_mut(p).life -= i16::from(amount),
        Recipient::Permanent(r) => {
            if !s.is_current(r) || s.object(r.slot).zone != Zone::Battlefield {
                return Ok(());
            }
            let types = s.derived(r.slot).types;
            let o = s.object_mut(r.slot);
            if types.contains(Types::PLANESWALKER) {
                o.counters.loyalty = o.counters.loyalty.saturating_sub(amount);
            }
            if types.contains(Types::CREATURE) {
                o.damage = o.damage.saturating_add(amount);
            }
        }
    }
    settle(
        s,
        t,
        sink,
        Event::Damage {
            source,
            to,
            amount,
            combat,
        },
    )
}

/// Puts counters on a permanent
/// ([CR 122.6](https://yawgatog.com/resources/magic-rules/#R1226)).
pub(crate) fn add_counters(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    object: ObjectRef,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !s.is_current(object) || amount == 0 {
        return Ok(());
    }
    let n = s.object_mut(object.slot).counters.get_mut(kind);
    *n = n.saturating_add(amount);
    s.flags.derived_dirty = true;
    settle(
        s,
        t,
        sink,
        Event::CountersAdded {
            object,
            kind,
            amount,
        },
    )
}

/// Removes up to `amount` counters of a kind from a permanent.
pub(crate) fn remove_counters(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    object: ObjectRef,
    kind: CounterKind,
    amount: u8,
) -> Result<(), Full> {
    if !s.is_current(object) {
        return Ok(());
    }
    // Read before writing: asking for the count to change would make `kind` the object's
    // long-tail kind even when it has none to remove.
    let amount = amount.min(s.object(object.slot).counters.get(kind));
    if amount == 0 {
        return Ok(());
    }
    *s.object_mut(object.slot).counters.get_mut(kind) -= amount;
    s.flags.derived_dirty = true;
    settle(
        s,
        t,
        sink,
        Event::CountersRemoved {
            object,
            kind,
            amount,
        },
    )
}

/// Taps or untaps a permanent.
///
/// [CR 701.26a](https://yawgatog.com/resources/magic-rules/#R70126a): "To tap a permanent,
/// turn it sideways from an upright position. Only untapped permanents can be tapped."
///
/// [CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b): "To untap a permanent,
/// rotate it back to the upright position from a sideways position. Only tapped permanents
/// can be untapped."
pub(crate) fn set_tapped(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    object: ObjectRef,
    tapped: bool,
) -> Result<(), Full> {
    if !s.is_current(object) || s.object(object.slot).flags.has(ObjectFlags::TAPPED) == tapped {
        return Ok(());
    }
    s.object_mut(object.slot)
        .flags
        .set(ObjectFlags::TAPPED, tapped);
    let e = if tapped {
        Event::Tap { object }
    } else {
        Event::Untap { object }
    };
    settle(s, t, sink, e)
}

/// How a token is created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenEntry {
    pub tapped: bool,
    /// Created attacking (CR 508.4): never declared, so no attack triggers.
    pub attacking: bool,
    /// A copy that is 1/1 by exception (an offspring token, CR 707.9b).
    pub one_one: bool,
    /// The face up of a copy of a double-faced permanent
    /// ([CR 707.8a](https://yawgatog.com/resources/magic-rules/#R7078a)): "If the token is a
    /// copy of a double-faced permanent with its back face up, the token enters the
    /// battlefield with its back face up."
    pub face: FaceIx,
}

/// Creates `count` tokens of the game-table card `card` under `controller`'s control.
///
/// [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "Some effects put tokens
/// onto the battlefield." [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The
/// player who creates a token is its owner." A token copy of a card uses the card's own
/// index: its copiable values are the card's
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)).
///
/// The tokens take free slots from the shared pool (architecture §4.2) and enter as one
/// event; then one `TokensCreated` event follows for "whenever you create one or more".
pub(crate) fn create_tokens(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    controller: Seat,
    card: CardIx,
    count: u8,
    how: TokenEntry,
) -> Result<Bitboard, Full> {
    let mut tokens = Bitboard::EMPTY;
    let mut free = token_slots().filter(|&slot| s.object(slot).zone == Zone::Nowhere);
    for _ in 0..count {
        let slot = free.next().ok_or(Full)?;
        tokens.insert(slot);
    }
    for slot in tokens.iter() {
        let o = s.object_mut(slot);
        o.card = card;
        o.owner = controller;
        o.base_controller = controller;
        o.flags = ObjectFlags(ObjectFlags::TOKEN);
        o.flags.set(ObjectFlags::ONE_ONE, how.one_one);
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
            face: how.face,
            ..Entry::default()
        },
    )?;
    settle(s, t, sink, Event::TokensCreated { controller, tokens })?;
    Ok(tokens)
}
