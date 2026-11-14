//! Triggered abilities: matching events, putting them on the stack in APNAP order, delayed
//! triggers, and resolving them (architecture §8.3, CR 603).

use std::cell::OnceCell;

use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, CastingKind, Face, FaceIx, Subject, Trigger, TriggerEvent, Types,
};
use crate::event::{Event, EventKind, Listens, Recipient, Sink};
use crate::filter::{Evaluate, Filter, is_for};
use crate::fixed::Full;
use crate::ids::{CardId, Seat};
use crate::layers;
use crate::layout::{Layout, Queue, index};
use crate::query::Copiable;
use crate::state::{
    AbilityRef, Lki, ObjectRef, PendingTrigger, PermanentTypes, StackEntry, StackItem, State,
    TriggerInfo, TurnStep, Zone,
};
use crate::table::GameTable;

/// The face a permanent has up, or a face-down one's blank characteristics: its copiable
/// values (CR 707.2).
pub(crate) fn face<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> &'static Face {
    s.copiable(slot).face(t)
}

/// The ability a pending trigger or stack entry names.
pub(crate) fn ability(
    t: &GameTable,
    card: impl CardId,
    face: FaceIx,
    ability: AbilityIx,
) -> &'static Ability {
    &t.def(card).face(face).abilities[ability.index()]
}

/// Matches an executed event against every triggered ability that listens to its kind: those of the
/// permanents on the battlefield and of the players' emblems ([CR 114.4]: "Abilities of emblems
/// function in the command zone"), then the delayed triggers waiting for a step.
///
/// The game's table has each card's triggered abilities under the kind of event they listen
/// to ([`GameTable::listeners`]), so an event reads only those of its kind, and where no
/// card of the game has one the whole check is an AND
/// ([`GameTable::listens_any`]).
///
/// [CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032): "Whenever a game event or game
/// state matches a triggered ability's trigger event, that ability automatically triggers."
///
/// [CR 114.4]: https://yawgatog.com/resources/magic-rules/#R1144
pub(crate) fn match_event<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    e: &Event<L>,
) -> Result<(), Full> {
    let Some(k) = e.kind() else {
        return Ok(());
    };
    let mask = Listens::event(k);
    if t.listens_any().intersects(mask) {
        // Only permanents can have changed what listens while the layer results are stale.
        let listening = s.listening().map_or(t.listens_any(), |listening| {
            debug_assert_eq!(
                listening,
                layers::listening(s, t).0,
                "the battlefield's listeners, as the layer results found them"
            );
            listening
        });
        if listening.intersects(mask) {
            for i in 0..s.battlefield().len() {
                let slot = s.battlefield()[i];
                let what = s.copiable(slot);
                if !what.may_listen(t).intersects(mask) {
                    continue;
                }
                let source = s.object_ref(slot);
                let controller = s.controller(slot);
                let (card, face) = what.record(t);
                for l in t.listeners(card, face, k) {
                    if let Some(info) = fires(s, t, l.trigger, Some(slot), controller, e) {
                        for _ in 0..times(e) {
                            enqueue(s, t, Some(source), card, face, l.ability, controller, info)?;
                        }
                    }
                }
            }
        }
        // Triggers wait in the order found, which the queue of them keeps: by seat.
        for player in s.players_in_seat_order(s.players()) {
            for index in 0..s.player(player).emblems().len() {
                let card = s.player(player).emblems()[index];
                if !Copiable::<L>::of_card(card).may_listen(t).intersects(mask) {
                    continue;
                }
                for l in t.listeners(card, FaceIx::FRONT, k) {
                    if let Some(info) = fires(s, t, l.trigger, None, player, e) {
                        for _ in 0..times(e) {
                            enqueue(s, t, None, card, FaceIx::FRONT, l.ability, player, info)?;
                        }
                    }
                }
            }
        }
    }
    if let Event::StepBegins { step, active } = *e {
        fire_delayed(s, t, step, active)?;
    }
    Ok(())
}

/// How many times a trigger that fires on `e` triggers: once for each card of a draw event, whose
/// cards are individual draws ([CR 603.2c]): "An ability triggers only once each time its trigger
/// event occurs. However, it can trigger repeatedly if one event contains multiple occurrences."
///
/// [CR 603.2c]: https://yawgatog.com/resources/magic-rules/#R6032c
fn times<L: Layout>(e: &Event<L>) -> u8 {
    match *e {
        Event::Draw { cards, .. } => cards,
        _ => 1,
    }
}

/// A count of objects as the amount a trigger carries, which is a byte: past 255, which
/// only a layout of more slots than [`crate::Small`] can count, it is 255 and never the
/// count's low byte.
fn counted(count: u32) -> u8 {
    u8::try_from(count).unwrap_or(u8::MAX)
}

/// Whether a trigger waiting for the beginning of a step or phase waits for `step` ([CR 500.6]):
/// "When a phase or step begins, any abilities that trigger 'at the beginning of' that phase or
/// step trigger."
///
/// [CR 500.6]: https://yawgatog.com/resources/magic-rules/#R5006
fn begins(event: TriggerEvent, step: TurnStep) -> bool {
    match event {
        TriggerEvent::Step(at) => at == step,
        // CR 505.1: "There are two main phases in a turn."
        TriggerEvent::MainPhase => {
            matches!(step, TurnStep::PrecombatMain | TurnStep::PostcombatMain)
        }
        _ => false,
    }
}

/// Whether a trigger of a source controlled by `you` fires on `e`, an event of the kind it
/// listens to, and with what: the event's object if it is the trigger's subject, the
/// event's player if the trigger is for them, and an amount.
///
/// Each kind of event says what it is about, and the subject and the players are read the
/// same way for every one ([`about`], [`how_many`], [`is_for`]). The card table has refused
/// every trigger this would not match ([`beryl_kernel::listener::listens_to`]).
fn fires<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    trigger: &Trigger,
    this: Option<L::Slot>,
    you: Seat,
    e: &Event<L>,
) -> Option<TriggerInfo<L>> {
    let info = |object: Option<ObjectRef<L>>, player: Seat, amount: u8| TriggerInfo {
        object,
        player,
        amount,
        lki: Lki::default(),
    };
    let whose = |player: Seat| is_for(&trigger.whose, player, you);
    let about = |object: ObjectRef<L>| about(s, t, trigger.subject, this, you, object);
    match (trigger.event, e) {
        // The amount is the X its spell was cast with
        // ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "If an object's
        // enters-the-battlefield triggered ability ... refers to X, and the spell that became
        // that object as it resolved had a value of X chosen for any of its costs, the value
        // of X for that ability is the same as the value of X for that spell".
        (TriggerEvent::Enters, &Event::ZoneChange { object, .. }) if about(object) => {
            Some(info(Some(object), you, s.x_of(object.slot)))
        }
        (TriggerEvent::Attacks, Event::Attack { attackers, .. })
            if this.is_some_and(|slot| attackers.contains(slot)) =>
        {
            Some(info(Some(s.object_ref(this?)), you, 0))
        }
        (TriggerEvent::PlayerAttacks, &Event::Attack { player, attackers }) if whose(player) => {
            let attacking = how_many(s, t, trigger.subject, attackers, you);
            (attacking > 0).then(|| info(None, player, counted(attacking)))
        }
        (
            TriggerEvent::DealsCombatDamageToPlayer,
            &Event::Damage {
                source,
                to: Recipient::Player(player),
                amount,
                combat: true,
            },
        ) if whose(player) && about(source) => Some(info(Some(source), player, amount)),
        (event, &Event::StepBegins { step, active }) if begins(event, step) && whose(active) => {
            Some(info(None, active, 0))
        }
        (TriggerEvent::CreatesTokens, &Event::TokensCreated { controller, tokens })
            if whose(controller) =>
        {
            let matching = how_many(s, t, trigger.subject, tokens, you);
            (matching > 0).then(|| info(None, controller, counted(matching)))
        }
        (
            TriggerEvent::Casts,
            &Event::Cast {
                spell, controller, ..
            },
        ) if whose(controller) && about(spell) => Some(info(Some(spell), controller, 0)),
        (TriggerEvent::Discards, &Event::ZoneChange { object, .. }) => {
            let discarder = s.object(object.slot).owner();
            (whose(discarder) && about(object)).then(|| info(Some(object), discarder, 0))
        }
        (TriggerEvent::Draws, &Event::Draw { player, .. }) if whose(player) => {
            Some(info(None, player, 0))
        }
        _ => None,
    }
}

/// Whether `object`, which an event is about, is a trigger's subject: the object its
/// ability is on, or one its filter accepts as it is now, read for the ability's controller.
fn about<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: Subject,
    this: Option<L::Slot>,
    you: Seat,
    object: ObjectRef<L>,
) -> bool {
    match subject {
        Subject::This => this == Some(object.slot) && s.is_current(object),
        Subject::A(Filter::Any) => true,
        Subject::A(filter) => s.is_current(object) && filter.matches(s, t, object.slot, you),
    }
}

/// How many of `objects`, the set an event is about, a trigger's subject accepts: those on
/// the battlefield its filter accepts, read for the ability's controller. "One or more"
/// triggers once, with the number.
fn how_many<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: Subject,
    objects: Bitboard<L::Slot>,
    you: Seat,
) -> u32 {
    let Subject::A(filter) = subject else {
        unreachable!("the card table refuses a set of objects that is this one");
    };
    objects
        .filter(|slot| {
            s.object(slot).zone() == Zone::Battlefield && filter.matches(s, t, slot, you)
        })
        .count()
}

/// Matches the leaves-the-battlefield abilities of a permanent about to move, which look back in
/// time ([CR 603.10a]), and captures its last-known information for their bodies (architecture §5).
/// The permanent is still on the battlefield, with every other permanent of the same event.
///
/// [CR 700.4](https://yawgatog.com/resources/magic-rules/#R7004): "The term dies means 'is put into
/// a graveyard from the battlefield.'"
///
/// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
pub(crate) fn match_leaving<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    slot: L::Slot,
    to: Zone,
) -> Result<(), Full> {
    let what = s.copiable(slot);
    if !what.may_listen(t).hears(EventKind::Dies) || to != Zone::Graveyard {
        return Ok(());
    }
    let lki = lki(s, t, slot);
    if !lki.types.types().contains(Types::CREATURE) {
        return Ok(());
    }
    let source = s.object_ref(slot);
    let (card, face) = what.record(t);
    // Each is "when this dies": the table has no other under the kind.
    for l in t.listeners(card, face, EventKind::Dies) {
        let info = TriggerInfo {
            object: Some(source),
            player: lki.controller,
            amount: 0,
            lki,
        };
        enqueue(
            s,
            t,
            Some(source),
            card,
            face,
            l.ability,
            lki.controller,
            info,
        )?;
    }
    Ok(())
}

/// A permanent's last-known information, from its current characteristics.
pub(crate) fn lki<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Lki<L> {
    let d = s.derived(slot);
    let o = s.object(slot);
    Lki {
        card: s.copiable(slot).record(t).0,
        power: d.power,
        toughness: d.toughness,
        types: PermanentTypes::of(d.types),
        controller: d.controller,
        token: o.is_token(),
    }
}

/// Records a triggered ability, if its intervening "if" holds now ([CR 603.4]): "When the trigger
/// event occurs, the ability checks whether the stated condition is true. The ability triggers only
/// if it is; otherwise it does nothing."
///
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
#[allow(clippy::too_many_arguments)]
fn enqueue<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    source: Option<ObjectRef<L>>,
    card: L::Card,
    face: FaceIx,
    ability: AbilityIx,
    controller: Seat,
    info: TriggerInfo<L>,
) -> Result<(), Full> {
    let pending = PendingTrigger {
        of: AbilityRef {
            source,
            card,
            face,
            ability,
            controller,
        },
        info,
    };
    if !crate::interpret::intervening_if(s, t, &pending, self::ability(t, card, face, ability)) {
        return Ok(());
    }
    s.queue_trigger(pending)
}

/// Fires the delayed triggers waiting for this step, each once ([CR 603.7b]).
///
/// [CR 603.7b]: https://yawgatog.com/resources/magic-rules/#R6037b
fn fire_delayed<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    step: TurnStep,
    active: Seat,
) -> Result<(), Full> {
    s.fire_delayed(
        |d| match ability(t, d.of.card, d.of.face, d.of.ability).trigger() {
            Some(trigger) if trigger.delayed => {
                begins(trigger.event, step) && is_for(&trigger.whose, active, d.of.controller)
            }
            _ => unreachable!("a delayed trigger's ability is a delayed one"),
        },
    )
}

/// The player who puts the next waiting trigger on the stack, and the triggers they choose
/// from: one of each set of equivalent ones (§7.2), as indices into the waiting triggers.
///
/// [CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP order,
/// puts each triggered ability they control ... on the stack in any order they choose."
pub(crate) fn next_to_stack<L: Layout>(s: &State<L>) -> Option<(Seat, L::Queue<L::Index, 64>)> {
    let player = s
        .players_in_turn_order(s.players())
        .find(|&p| s.triggers().iter().any(|tr| tr.of.controller == p))?;
    let mut kept: L::Queue<L::Index, 64> = Default::default();
    let mut firsts: L::Queue<PendingTrigger<L>, 64> = Default::default();
    // Found only once two triggers of one ability have different sources or objects.
    let named = OnceCell::new();
    for (i, tr) in s.triggers().iter().enumerate() {
        if tr.of.controller != player || firsts.iter().any(|k| equivalent(s, &named, k, tr)) {
            continue;
        }
        // The waiting triggers never exceed the capacity of these lists, and the layout's
        // index names every one.
        firsts.push_bounded(*tr);
        kept.push_bounded(index::<L>(i));
    }
    Some((player, kept))
}

/// Triggers of the same ability from interchangeable sources, on the same event with
/// interchangeable objects, lead to the same game whatever their order (§7.2).
///
/// Two objects are interchangeable here as two candidates are (`engine::collapse`): alike
/// in themselves ([`State::signature`]) and named by no other record of the state
/// ([`State::named`]). A waiting trigger names its own source, so the waiting triggers are
/// held to that rule another way: with one trigger's source and object renamed to the
/// other's and back, they must be the same triggers. Two abilities that each watch one of
/// two twins leave the twins alike; one ability waiting to act on one twin alone tells
/// them apart, as it will once it is on the stack.
fn equivalent<L: Layout>(
    s: &State<L>,
    named: &OnceCell<Bitboard<L::Slot>>,
    a: &PendingTrigger<L>,
    b: &PendingTrigger<L>,
) -> bool {
    if (a.of.card, a.of.face, a.of.ability, a.of.controller)
        != (b.of.card, b.of.face, b.of.ability, b.of.controller)
        || (a.info.player, a.info.amount, a.info.lki) != (b.info.player, b.info.amount, b.info.lki)
    {
        return false;
    }
    let pairs = [(a.of.source, b.of.source), (a.info.object, b.info.object)];
    if pairs.iter().all(|(x, y)| x == y) {
        return true;
    }
    let named = *named.get_or_init(|| s.named_apart_from_waiting_triggers());
    let alike = |x: L::Slot, y: L::Slot| {
        !named.contains(x) && !named.contains(y) && s.signature(x) == s.signature(y)
    };
    let interchangeable = |&(x, y): &(Option<ObjectRef<L>>, Option<ObjectRef<L>>)| match (x, y) {
        (Some(x), Some(y)) => x == y || alike(x.slot, y.slot),
        _ => x == y,
    };
    if !pairs.iter().all(interchangeable) {
        return false;
    }
    // The renaming that makes each trigger the other: an object of a pair for its partner.
    let renamed = |r: Option<ObjectRef<L>>| {
        let partner = |&(x, y): &(Option<ObjectRef<L>>, Option<ObjectRef<L>>)| match r {
            _ if x == y => None,
            r if r == x => Some(y),
            r if r == y => Some(x),
            _ => None,
        };
        pairs.iter().find_map(partner).unwrap_or(r)
    };
    let rename = |tr: &PendingTrigger<L>| PendingTrigger {
        of: AbilityRef {
            source: renamed(tr.of.source),
            ..tr.of
        },
        info: TriggerInfo {
            object: renamed(tr.info.object),
            ..tr.info
        },
    };
    // It is one renaming only if it makes each of the other: a creature that saw itself
    // enter is no twin of one that saw another.
    if rename(a) != *b || rename(b) != *a {
        return false;
    }
    let waiting = s.triggers();
    let count = |tr: &PendingTrigger<L>| waiting.iter().filter(|w| *w == tr).count();
    waiting.iter().all(|tr| {
        let other = rename(tr);
        other == *tr || count(tr) == count(&other)
    })
}

/// Puts the waiting trigger at `index` on the stack ([CR 603.3]): "The ability becomes the topmost
/// object on the stack." Then its targets are chosen, through the casting machine (CR 603.3d).
///
/// [CR 603.3]: https://yawgatog.com/resources/magic-rules/#R6033
pub(crate) fn put_on_stack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    index: L::Index,
) -> Result<(), Full> {
    let tr = s.take_trigger(index.into());
    // An enters ability's X is its spell's (CR 107.3m), captured as it triggered.
    let entering = ability(t, tr.of.card, tr.of.face, tr.of.ability)
        .trigger()
        .is_some_and(|trigger| trigger.is_this(TriggerEvent::Enters));
    s.put_on_stack(
        sink,
        StackEntry {
            x: if entering { tr.info.amount } else { 0 },
            ..StackEntry::new(StackItem::Triggered {
                of: tr.of,
                info: tr.info,
            })
        },
    )?;
    crate::cast::target_trigger(s, t, sink)
}

/// Creates a delayed triggered ability ([CR 603.7]).
///
/// [CR 603.7]: https://yawgatog.com/resources/magic-rules/#R6037
pub(crate) fn delay<L: Layout>(s: &mut State<L>, trigger: PendingTrigger<L>) -> Result<(), Full> {
    s.delay_trigger(trigger)
}

/// Whether a cost of `kind` was paid for a permanent's spell, for an intervening "if" of its
/// enters ability (CR 603.4). The permanent remembers it from the moment it enters and never
/// otherwise while it stays; once it has left, the answer is what it was as the ability
/// triggered, and it triggered only if the cost was paid.
pub(crate) fn entered_with<L: Layout>(
    s: &State<L>,
    tr: &PendingTrigger<L>,
    kind: CastingKind,
) -> bool {
    match tr.of.source {
        Some(r) if s.is_current(r) => s.paid(r.slot, kind),
        _ => true,
    }
}
