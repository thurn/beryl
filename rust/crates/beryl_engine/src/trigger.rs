//! Triggered abilities: matching events, putting them on the stack in APNAP order, delayed
//! triggers, and resolving them (architecture §8.3, CR 603).

use crate::card::{
    Ability, AbilityIx, CastingKind, Delayed, Face, FaceAbility, FaceIx, Trigger, Types,
};
use crate::event::{Event, Reason, Recipient, Sink, kind};
use crate::filter;
use crate::fixed::Full;
use crate::ids::{CardId, Seat};
use crate::layers;
use crate::layout::{Layout, Queue, index};
use crate::query::Copiable;
use crate::state::{
    Lki, ObjectFlags, ObjectRef, PendingTrigger, StackEntry, State, TriggerInfo, TurnStep, Zone,
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

/// Matches an executed event against every triggered ability that could listen to it: the
/// permanents on the battlefield and the players' emblems
/// ([CR 114.4](https://yawgatog.com/resources/magic-rules/#R1144): "Abilities of emblems
/// function in the command zone"), then the delayed triggers waiting for a step.
///
/// [CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032): "Whenever a game event or
/// game state matches a triggered ability's trigger event, that ability automatically
/// triggers."
pub(crate) fn match_event<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    e: &Event<L>,
) -> Result<(), Full> {
    let k = e.kind();
    if k == 0 {
        return Ok(());
    }
    if t.listens_any() & k != 0 {
        // Only permanents can have changed what listens while the layer results are stale.
        let listening = s.listening().map_or(t.listens_any(), |listening| {
            debug_assert_eq!(
                listening,
                layers::listening(s, t),
                "the battlefield's listeners, as the layer results found them"
            );
            listening
        });
        if listening & k != 0 {
            for i in 0..s.battlefield().len() {
                let slot = s.battlefield()[i];
                let what = s.copiable(slot);
                if what.may_listen(t) & k == 0 {
                    continue;
                }
                let source = s.object_ref(slot);
                let controller = s.controller(slot);
                let (card, face) = what.record(t);
                for (a, ability) in face_abilities(t, card, face) {
                    if let Some(info) = fires(s, ability.trigger, Some(slot), controller, e) {
                        for _ in 0..times(ability.trigger, e) {
                            enqueue(s, t, Some(source), card, face, a, controller, info)?;
                        }
                    }
                }
            }
        }
        // Triggers wait in the order found, which the queue of them keeps: by seat.
        for player in s.players_in_seat_order(s.players()) {
            for index in 0..s.player(player).emblems().len() {
                let card = s.player(player).emblems()[index];
                if Copiable::<L>::of_card(card).may_listen(t) & k == 0 {
                    continue;
                }
                for (a, ability) in face_abilities(t, card, FaceIx::FRONT) {
                    if let Some(info) = fires(s, ability.trigger, None, player, e) {
                        for _ in 0..times(ability.trigger, e) {
                            enqueue(s, t, None, card, FaceIx::FRONT, a, player, info)?;
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

/// How many times a trigger that fires on `e` triggers: once for each card of a draw event,
/// whose cards are individual draws
/// ([CR 603.2c](https://yawgatog.com/resources/magic-rules/#R6032c)): "An ability triggers
/// only once each time its trigger event occurs. However, it can trigger repeatedly if one
/// event contains multiple occurrences."
fn times<L: Layout>(trigger: Trigger, e: &Event<L>) -> u8 {
    match (trigger, e.clone()) {
        (Trigger::YouDraw, Event::Draw { cards, .. }) => cards,
        _ => 1,
    }
}

fn face_abilities(
    t: &GameTable,
    card: impl CardId,
    face: FaceIx,
) -> impl Iterator<Item = (AbilityIx, &'static Ability)> {
    t.def(card)
        .face(face)
        .abilities
        .iter()
        .enumerate()
        .map(|(i, a)| (AbilityIx::new(i as u8), a))
}

/// Whether a trigger of a source controlled by `you` fires on `e`, and with what.
fn fires<L: Layout>(
    s: &State<L>,
    trigger: Trigger,
    this: Option<L::Slot>,
    you: Seat,
    e: &Event<L>,
) -> Option<TriggerInfo<L>> {
    let is_this = |r: ObjectRef<L>| this == Some(r.slot) && s.is_current(r);
    let info = |object: Option<ObjectRef<L>>, player: Seat, amount: u8| TriggerInfo {
        object,
        player,
        amount,
        lki: Lki::default(),
    };
    match (trigger, e.clone()) {
        // The amount is the X its spell was cast with
        // ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "If an object's
        // enters-the-battlefield triggered ability ... refers to X, and the spell that became
        // that object as it resolved had a value of X chosen for any of its costs, the value
        // of X for that ability is the same as the value of X for that spell".
        (Trigger::Enters, Event::ZoneChange { object, .. }) if is_this(object) => {
            Some(info(Some(object), you, s.x_of(object.slot)))
        }
        (Trigger::Attacks, Event::Attack { attackers, .. })
            if this.is_some_and(|slot| attackers.contains(slot)) =>
        {
            Some(info(Some(s.object_ref(this?)), you, 0))
        }
        (Trigger::YouAttack, Event::Attack { player, attackers }) if player == you => {
            Some(info(None, you, attackers.count() as u8))
        }
        (
            Trigger::CombatDamageToPlayer,
            Event::Damage {
                source,
                to: Recipient::Player(player),
                amount,
                combat: true,
            },
        ) if is_this(source) => Some(info(Some(source), player, amount)),
        (Trigger::Step(step, whose), Event::StepBegins { step: now, active })
            if step == now && filter::is_whose(whose, active, you) =>
        {
            Some(info(None, active, 0))
        }
        (Trigger::YouCreateTokens(types), Event::TokensCreated { controller, tokens })
            if controller == you =>
        {
            let matching = tokens
                .filter(|slot| {
                    s.object(slot).zone() == Zone::Battlefield
                        && s.derived(slot).types.intersects(types)
                })
                .count();
            (matching > 0).then(|| info(None, you, matching as u8))
        }
        (
            Trigger::YouCast,
            Event::Cast {
                spell, controller, ..
            },
        ) if controller == you => Some(info(Some(spell), you, 0)),
        (
            Trigger::YouDiscard,
            Event::ZoneChange {
                object,
                reason: Reason::Discard,
                ..
            },
        ) if s.object(object.slot).owner() == you => Some(info(Some(object), you, 0)),
        (Trigger::YouDraw, Event::Draw { player, .. }) if player == you => Some(info(None, you, 0)),
        _ => None,
    }
}

/// Matches the leaves-the-battlefield abilities of a permanent about to move, which look
/// back in time ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)), and
/// captures its last-known information for their bodies (architecture §5). The permanent
/// is still on the battlefield, with every other permanent of the same event.
///
/// [CR 700.4](https://yawgatog.com/resources/magic-rules/#R7004): "The term dies means 'is
/// put into a graveyard from the battlefield.'"
pub(crate) fn match_leaving<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    slot: L::Slot,
    to: Zone,
) -> Result<(), Full> {
    let what = s.copiable(slot);
    if what.may_listen(t) & kind::DIES == 0 || to != Zone::Graveyard {
        return Ok(());
    }
    let lki = lki(s, t, slot);
    if !lki.types.contains(Types::CREATURE) {
        return Ok(());
    }
    let source = s.object_ref(slot);
    let (card, face) = what.record(t);
    for (a, ability) in face_abilities(t, card, face) {
        if ability.trigger == Trigger::Dies {
            let info = TriggerInfo {
                object: Some(source),
                player: lki.controller,
                amount: 0,
                lki,
            };
            enqueue(s, t, Some(source), card, face, a, lki.controller, info)?;
        }
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
        types: d.types,
        controller: d.controller,
        token: o.flags().has(ObjectFlags::TOKEN),
    }
}

/// Records a triggered ability, if its intervening "if" holds now
/// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "When the trigger event
/// occurs, the ability checks whether the stated condition is true. The ability triggers
/// only if it is; otherwise it does nothing."
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
        source,
        card,
        face,
        ability,
        controller,
        info,
    };
    if !crate::interpret::intervening_if(s, t, &pending, self::ability(t, card, face, ability)) {
        return Ok(());
    }
    s.queue_trigger(pending)
}

/// Fires the delayed triggers waiting for this step, each once
/// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)).
fn fire_delayed<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    step: TurnStep,
    active: Seat,
) -> Result<(), Full> {
    s.fire_delayed(|d| match ability(t, d.card, d.face, d.ability).trigger {
        Trigger::Delayed(Delayed::YourNextMainPhase) => {
            matches!(step, TurnStep::PrecombatMain | TurnStep::PostcombatMain)
                && active == d.controller
        }
        Trigger::Delayed(Delayed::NextEndStep) => step == TurnStep::End,
        _ => unreachable!("a delayed trigger's ability is a delayed one"),
    })
}

/// The player who puts the next waiting trigger on the stack, and the triggers they choose
/// from: one of each set of equivalent ones (§7.2), as indices into the waiting triggers.
///
/// [CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
/// order, puts each triggered ability they control ... on the stack in any order they
/// choose."
pub(crate) fn next_to_stack<L: Layout>(s: &State<L>) -> Option<(Seat, L::Queue<L::Index, 64>)> {
    let player = s
        .players_in_turn_order(s.players())
        .find(|&p| s.triggers().iter().any(|tr| tr.controller == p))?;
    let mut kept: L::Queue<L::Index, 64> = Default::default();
    let mut firsts: L::Queue<PendingTrigger<L>, 64> = Default::default();
    for (i, tr) in s.triggers().iter().enumerate() {
        if tr.controller != player || firsts.iter().any(|k| equivalent(s, k, tr)) {
            continue;
        }
        // The waiting triggers never exceed the capacity of these lists, and the layout's
        // index names every one.
        let _ = firsts.push(*tr);
        let _ = kept.push(index::<L>(i));
    }
    Some((player, kept))
}

/// Triggers of the same ability from interchangeable sources, on the same event with
/// interchangeable objects, lead to the same game whatever their order (§7.2).
fn equivalent<L: Layout>(s: &State<L>, a: &PendingTrigger<L>, b: &PendingTrigger<L>) -> bool {
    let interchangeable = |x: Option<ObjectRef<L>>, y: Option<ObjectRef<L>>| match (x, y) {
        (Some(x), Some(y)) => x == y || s.signature(x.slot) == s.signature(y.slot),
        _ => x == y,
    };
    interchangeable(a.source, b.source)
        && interchangeable(a.info.object, b.info.object)
        && (a.card, a.face, a.ability, a.controller) == (b.card, b.face, b.ability, b.controller)
        && (a.info.player, a.info.amount, a.info.lki) == (b.info.player, b.info.amount, b.info.lki)
}

/// Puts the waiting trigger at `index` on the stack
/// ([CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033)): "The ability becomes the
/// topmost object on the stack."
/// Then its targets are chosen, through the casting machine (CR 603.3d).
pub(crate) fn put_on_stack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    index: L::Index,
) -> Result<(), Full> {
    let tr = s.take_trigger(index.into());
    // An enters ability's X is its spell's (CR 107.3m), captured as it triggered.
    let entering = ability(t, tr.card, tr.face, tr.ability).trigger == Trigger::Enters;
    s.put_on_stack(StackEntry {
        object: tr.source,
        controller: tr.controller,
        ability: Some(FaceAbility::Own(tr.ability)),
        card: tr.card,
        face: tr.face,
        info: tr.info,
        x: if entering { tr.info.amount } else { 0 },
        ..StackEntry::default()
    })?;
    crate::cast::target_trigger(s, t, sink)
}

/// Creates a delayed triggered ability ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
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
    match tr.source {
        Some(r) if s.is_current(r) => s.paid(r.slot, kind),
        _ => true,
    }
}
