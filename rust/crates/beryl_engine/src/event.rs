//! Events and the pipeline every game action goes through (architecture §8.1, §8.2,
//! decision D10).
//!
//! A primitive performs its mutation through here: zone changes get replacement effects,
//! look-back trigger matching with last-known information, execution, a layer refresh, then
//! trigger matching and the sink; every other event is executed by its primitive and then
//! [`settle`]d the same way. The cube's other replacement and prevention effects (damage,
//! draws) arrive with the cards that create them; none of the Milestone 1 cards does.

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, Replacement, Types};
use crate::fixed::{Fixed, Full};
use crate::layers;
use crate::state::{
    Continuous, CounterKind, NONE, ObjectFlags, ObjectRef, State, TOKEN_SLOTS, TurnStep, Zone,
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
}

/// The player or permanent dealt damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Recipient {
    Player(u8),
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
        player: u8,
        cards: u8,
    },
    LifeGain {
        player: u8,
        amount: u8,
    },
    LifeLoss {
        player: u8,
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
        controller: u8,
        tokens: Bitboard,
    },
    /// A spell was cast ([CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i)).
    Cast {
        spell: ObjectRef,
        controller: u8,
    },
    /// A player declared attackers ([CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)).
    Attack {
        player: u8,
        attackers: Bitboard,
    },
    /// A step or phase began ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)).
    StepBegins {
        step: TurnStep,
        active: u8,
    },
    GainControl {
        object: ObjectRef,
        player: u8,
    },
    /// A coin flip ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "If the
    /// call matches the result, the player wins the flip."
    CoinFlip {
        player: u8,
        won: bool,
    },
    Lose {
        player: u8,
    },
    Win {
        player: u8,
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
}

impl Event {
    /// The listener kind of this event, or 0 if no trigger listens to it.
    pub(crate) fn kind(&self) -> u16 {
        match self {
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            } => kind::ENTERS,
            Event::Attack { .. } => kind::ATTACK,
            Event::Damage { .. } => kind::DAMAGE,
            Event::StepBegins { .. } => kind::STEP,
            Event::TokensCreated { .. } => kind::TOKENS,
            Event::Cast { .. } => kind::CAST,
            _ => 0,
        }
    }
}

/// Where executed events go for the UI or a replay (§8.2, step 6). Search leaves it off.
#[derive(Debug, Default)]
pub struct Sink {
    events: Vec<Event>,
    on: bool,
}

impl Sink {
    /// A sink that drops every event.
    pub fn off() -> Self {
        Self::default()
    }

    /// A sink that keeps every event until drained.
    pub fn recording() -> Self {
        Self {
            events: Vec::new(),
            on: true,
        }
    }

    /// The events since the last drain, oldest first.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub(crate) fn push(&mut self, e: Event) {
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
    pub controller: Option<u8>,
    /// The face up.
    pub face: u8,
    /// Counters it enters with ("enters with N time counters").
    pub counters: Option<(CounterKind, u8)>,
    /// Its controller chose to pay the life an "unless you pay N life" replacement asks.
    pub pay_life: bool,
}

/// Executes the remaining steps of the pipeline for an event its primitive has executed:
/// a layer refresh so nothing reads `derived` dirty, trigger matching (§8.2, step 5), and
/// the sink.
pub(crate) fn settle(s: &mut State, t: &GameTable, sink: &mut Sink, e: Event) -> Result<(), Full> {
    layers::refresh(s, t);
    trigger::match_event(s, t, &e)?;
    sink.push(e);
    Ok(())
}

/// Moves one object.
pub(crate) fn move_one(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    slot: u8,
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
///    are on the battlefield
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
    layers::refresh(s, t);
    let mut moves: Fixed<(u8, Zone, Zone, Entry), 128> = Fixed::new();
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
        zone::move_to_place(s, slot, dest, Place::Top)?;
        if dest == Zone::Battlefield {
            let o = s.object_mut(slot);
            o.flags.set(ObjectFlags::TAPPED, entry.tapped);
            o.flags.set(ObjectFlags::ATTACKING, entry.attacking);
            o.face = entry.face;
            if let Some(controller) = entry.controller {
                o.controller = controller;
            }
            if let Some((kind, n)) = entry.counters {
                *o.counters.get_mut(kind) = n;
            }
        }
        sink.push(Event::ZoneChange {
            object: s.object_ref(slot),
            from,
            to: dest,
            reason,
        });
    }
    layers::refresh(s, t);
    for &(slot, from, dest, _) in &moves {
        if dest == Zone::Battlefield {
            let e = Event::ZoneChange {
                object: s.object_ref(slot),
                from,
                to: dest,
                reason,
            };
            trigger::match_event(s, t, &e)?;
        }
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
fn destination(s: &State, slot: u8, to: Zone) -> Zone {
    let r = s.object_ref(slot);
    let exiled = to == Zone::Graveyard
        && s.effects
            .iter()
            .any(|e| e.affected == r && e.effect == Continuous::ExileInsteadOfGraveyard);
    if exiled { Zone::Exile } else { to }
}

/// The objects exiled until the permanent in `slot` leaves the battlefield.
fn exiled_until(s: &State, slot: u8) -> Bitboard {
    (s.zone(0, Zone::Exile) | s.zone(1, Zone::Exile))
        .iter()
        .filter(|&x| {
            let o = s.object(x);
            o.flags.has(ObjectFlags::EXILED_UNTIL) && o.linked == slot
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
fn enter_replacements(s: &State, t: &GameTable, slot: u8, mut entry: Entry) -> Entry {
    let o = s.object(slot);
    let face = &t.def(o.card).faces[usize::from(entry.face)];
    let controller = entry.controller.unwrap_or(o.owner);
    for ability in face.abilities {
        if ability.kind != AbilityKind::Replacement {
            continue;
        }
        match ability.replacement {
            Replacement::None => {}
            Replacement::EntersTapped => entry.tapped = true,
            Replacement::EntersTappedUnlessFewLands(n) => {
                // Not yet on the battlefield, so every land counted is another land.
                let lands = s
                    .battlefield()
                    .iter()
                    .filter(|&&p| {
                        let d = s.derived(p);
                        d.controller == controller && Types(d.types).contains(Types::LAND)
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
    slot: u8,
    entry: Entry,
) -> Result<(), Full> {
    let o = s.object(slot);
    let face = &t.def(o.card).faces[usize::from(entry.face)];
    let player = entry.controller.unwrap_or(o.owner);
    for ability in face.abilities {
        if let Replacement::EntersTappedUnlessPayLife(n) = ability.replacement {
            lose_life(s, t, sink, player, n)?;
        }
    }
    Ok(())
}

/// The life an entering replacement of the face asks for, if any.
pub(crate) fn life_to_enter(t: &GameTable, card: u8, face: u8) -> Option<u8> {
    t.def(card).faces[usize::from(face)]
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
    player: u8,
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
    player: u8,
    amount: u8,
) -> Result<(), Full> {
    s.player_mut(player).life += i16::from(amount);
    settle(s, t, sink, Event::LifeGain { player, amount })
}

/// A player draws cards, one at a time
/// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a player is instructed
/// to draw multiple cards, that player performs that many individual card draws."
pub(crate) fn draw(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: u8,
    cards: u8,
) -> Result<(), Full> {
    zone::draw_n(s, player, cards)?;
    settle(s, t, sink, Event::Draw { player, cards })
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
            let types = Types(s.derived(r.slot).types);
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
    controller: u8,
    card: u8,
    count: u8,
    how: TokenEntry,
) -> Result<Bitboard, Full> {
    let mut tokens = Bitboard::EMPTY;
    // Slot NONE is never used: it means "no object" in references.
    let mut free = (TOKEN_SLOTS..NONE).filter(|&slot| s.object(slot).zone == Zone::Nowhere);
    for _ in 0..count {
        let slot = free.next().ok_or(Full)?;
        tokens.insert(slot);
    }
    for slot in tokens.iter() {
        let o = s.object_mut(slot);
        o.card = card;
        o.owner = controller;
        o.controller = controller;
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
            ..Entry::default()
        },
    )?;
    settle(s, t, sink, Event::TokensCreated { controller, tokens })?;
    Ok(tokens)
}

/// The object in a slot, as a reference that names no object, for an emblem's triggers.
pub(crate) const NO_OBJECT: ObjectRef = ObjectRef {
    slot: NONE,
    incarnation: 0,
};
