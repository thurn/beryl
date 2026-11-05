//! Triggered abilities: matching events, putting them on the stack in APNAP order, delayed
//! triggers, and resolving them (architecture §8.3, CR 603).

use crate::bitboard::Bitboard;
use crate::card::{Ability, Delayed, Face, Trigger, Types};
use crate::event::{Event, NO_OBJECT, Recipient, Sink, kind};
use crate::filter::Whose;
use crate::fixed::Full;
use crate::state::{
    Lki, NONE, ObjectFlags, ObjectRef, PendingTrigger, StackEntry, State, TriggerInfo, TurnStep,
    Zone,
};
use crate::table::GameTable;
use crate::turn::apnap;

/// The printed face of a permanent, or of the card a token copies.
pub(crate) fn face(s: &State, t: &GameTable, slot: u8) -> &'static Face {
    let o = s.object(slot);
    &t.def(o.card).faces[usize::from(o.face)]
}

/// The ability a pending trigger or stack entry names.
pub(crate) fn ability(t: &GameTable, card: u8, face: u8, ability: u8) -> &'static Ability {
    &t.def(card).faces[usize::from(face)].abilities[usize::from(ability)]
}

/// Matches an executed event against every triggered ability that could listen to it: the
/// permanents on the battlefield and the players' emblems
/// ([CR 114.4](https://yawgatog.com/resources/magic-rules/#R1144): "Abilities of emblems
/// function in the command zone"), then the delayed triggers waiting for a step.
///
/// [CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032): "Whenever a game event or
/// game state matches a triggered ability's trigger event, that ability automatically
/// triggers."
pub(crate) fn match_event(s: &mut State, t: &GameTable, e: &Event) -> Result<(), Full> {
    let k = e.kind();
    if k == 0 {
        return Ok(());
    }
    if t.listens_any() & k != 0 {
        for i in 0..s.battlefield.len() {
            let slot = s.battlefield.as_slice()[i];
            let card = s.object(slot).card;
            if t.listens(card) & k == 0 {
                continue;
            }
            let source = s.object_ref(slot);
            let controller = s.controller(slot);
            let face = s.object(slot).face;
            for (a, ability) in face_abilities(t, card, face) {
                if let Some(info) = fires(s, ability.trigger, Some(slot), controller, e) {
                    enqueue(s, t, source, card, face, a, controller, info)?;
                }
            }
        }
        for player in 0..2u8 {
            let emblems = s.player(player).emblems;
            for &card in &emblems {
                if t.listens(card) & k == 0 {
                    continue;
                }
                for (a, ability) in face_abilities(t, card, 0) {
                    if let Some(info) = fires(s, ability.trigger, None, player, e) {
                        enqueue(s, t, NO_OBJECT, card, 0, a, player, info)?;
                    }
                }
            }
        }
    }
    if let Event::StepBegins { step, active } = *e {
        fire_delayed(s, step, active)?;
    }
    Ok(())
}

fn face_abilities(
    t: &GameTable,
    card: u8,
    face: u8,
) -> impl Iterator<Item = (u8, &'static Ability)> {
    t.def(card).faces[usize::from(face)]
        .abilities
        .iter()
        .enumerate()
        .map(|(i, a)| (i as u8, a))
}

/// Whether a trigger of a source controlled by `you` fires on `e`, and with what.
fn fires(s: &State, trigger: Trigger, this: Option<u8>, you: u8, e: &Event) -> Option<TriggerInfo> {
    let is_this = |r: ObjectRef| this == Some(r.slot) && s.is_current(r);
    let info = |object: ObjectRef, player: u8, amount: u8| TriggerInfo {
        object,
        player,
        amount,
        lki: Lki::default(),
    };
    match (trigger, *e) {
        // The amount is the X its spell was cast with
        // ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "If an object's
        // enters-the-battlefield triggered ability ... refers to X, and the spell that became
        // that object as it resolved had a value of X chosen for any of its costs, the value
        // of X for that ability is the same as the value of X for that spell".
        (Trigger::Enters, Event::ZoneChange { object, .. }) if is_this(object) => {
            Some(info(object, you, s.x_of(object.slot)))
        }
        (Trigger::Attacks, Event::Attack { attackers, .. })
            if this.is_some_and(|slot| attackers.contains(slot)) =>
        {
            Some(info(s.object_ref(this?), you, 0))
        }
        (Trigger::YouAttack, Event::Attack { player, attackers }) if player == you => {
            Some(info(NO_OBJECT, you, attackers.count() as u8))
        }
        (
            Trigger::CombatDamageToPlayer,
            Event::Damage {
                source,
                to: Recipient::Player(player),
                amount,
                combat: true,
            },
        ) if is_this(source) => Some(info(source, player, amount)),
        (Trigger::Step(step, whose), Event::StepBegins { step: now, active })
            if step == now && whose_turn(whose, you, active) =>
        {
            Some(info(NO_OBJECT, active, 0))
        }
        (Trigger::YouCreateTokens(types), Event::TokensCreated { controller, tokens })
            if controller == you =>
        {
            let matching = tokens
                .iter()
                .filter(|&slot| {
                    s.object(slot).zone == Zone::Battlefield
                        && Types(s.derived(slot).types).intersects(types)
                })
                .count();
            (matching > 0).then(|| info(NO_OBJECT, you, matching as u8))
        }
        (Trigger::YouCast, Event::Cast { spell, controller }) if controller == you => {
            Some(info(spell, you, 0))
        }
        _ => None,
    }
}

fn whose_turn(whose: Whose, you: u8, active: u8) -> bool {
    match whose {
        Whose::You => active == you,
        Whose::Opponent => active != you,
    }
}

/// Matches the leaves-the-battlefield abilities of a permanent about to move, which look
/// back in time ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)), and
/// captures its last-known information for their bodies (architecture §5). The permanent
/// is still on the battlefield, with every other permanent of the same event.
///
/// [CR 700.4](https://yawgatog.com/resources/magic-rules/#R7004): "The term dies means 'is
/// put into a graveyard from the battlefield.'"
pub(crate) fn match_leaving(s: &mut State, t: &GameTable, slot: u8, to: Zone) -> Result<(), Full> {
    let card = s.object(slot).card;
    if t.listens(card) & kind::DIES == 0 || to != Zone::Graveyard {
        return Ok(());
    }
    let lki = lki(s, slot);
    if !lki.types.contains(Types::CREATURE) {
        return Ok(());
    }
    let source = s.object_ref(slot);
    let face = s.object(slot).face;
    for (a, ability) in face_abilities(t, card, face) {
        if ability.trigger == Trigger::Dies {
            let info = TriggerInfo {
                object: source,
                player: lki.controller,
                amount: 0,
                lki,
            };
            enqueue(s, t, source, card, face, a, lki.controller, info)?;
        }
    }
    Ok(())
}

/// A permanent's last-known information, from its current characteristics.
pub(crate) fn lki(s: &State, slot: u8) -> Lki {
    let d = s.derived(slot);
    let o = s.object(slot);
    Lki {
        card: o.card,
        power: d.power,
        toughness: d.toughness,
        types: Types(d.types),
        controller: d.controller,
        token: o.flags.has(ObjectFlags::TOKEN),
    }
}

/// Records a triggered ability, if its intervening "if" holds now
/// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "When the trigger event
/// occurs, the ability checks whether the stated condition is true. The ability triggers
/// only if it is; otherwise it does nothing."
#[allow(clippy::too_many_arguments)]
fn enqueue(
    s: &mut State,
    t: &GameTable,
    source: ObjectRef,
    card: u8,
    face: u8,
    ability: u8,
    controller: u8,
    info: TriggerInfo,
) -> Result<(), Full> {
    let pending = PendingTrigger {
        source,
        card,
        face,
        ability,
        controller,
        info,
    };
    if let Some(condition) = self::ability(t, card, face, ability).condition
        && !condition(s, t, &pending)
    {
        return Ok(());
    }
    s.triggers.push(pending)
}

/// Fires the delayed triggers waiting for this step, each once
/// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)).
fn fire_delayed(s: &mut State, step: TurnStep, active: u8) -> Result<(), Full> {
    let mut i = 0;
    while i < s.delayed.len() {
        let d = s.delayed.as_slice()[i];
        let due = match d.at {
            Delayed::YourNextMainPhase => {
                matches!(step, TurnStep::PrecombatMain | TurnStep::PostcombatMain)
                    && active == d.trigger.controller
            }
        };
        if due {
            s.delayed.remove(i);
            s.triggers.push(d.trigger)?;
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// The player who puts the next waiting trigger on the stack, and the triggers they choose
/// from: one of each set of equivalent ones (§7.2), as indices into the waiting triggers.
///
/// [CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
/// order, puts each triggered ability they control ... on the stack in any order they
/// choose."
pub(crate) fn next_to_stack(s: &State) -> Option<(u8, Bitboard)> {
    let player = apnap(s)
        .into_iter()
        .find(|&p| s.triggers.iter().any(|tr| tr.controller == p))?;
    let mut kept = Bitboard::EMPTY;
    let mut firsts: crate::fixed::Fixed<PendingTrigger, 64> = crate::fixed::Fixed::new();
    for (i, tr) in s.triggers.iter().enumerate() {
        if tr.controller != player || firsts.iter().any(|k| equivalent(s, k, tr)) {
            continue;
        }
        // The waiting triggers never exceed the capacity of this list.
        let _ = firsts.push(*tr);
        kept.insert(i as u8);
    }
    Some((player, kept))
}

/// Triggers of the same ability from interchangeable sources, on the same event with
/// interchangeable objects, lead to the same game whatever their order (§7.2).
fn equivalent(s: &State, a: &PendingTrigger, b: &PendingTrigger) -> bool {
    let interchangeable = |x: ObjectRef, y: ObjectRef| {
        x == y
            || (x.slot != NONE
                && y.slot != NONE
                && crate::zone::signature(s, x.slot) == crate::zone::signature(s, y.slot))
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
pub(crate) fn put_on_stack(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    index: u8,
) -> Result<(), Full> {
    let tr = s.triggers.remove(usize::from(index));
    // An enters ability's X is its spell's (CR 107.3m), captured as it triggered.
    let entering = ability(t, tr.card, tr.face, tr.ability).trigger == Trigger::Enters;
    s.stack.push(StackEntry {
        object: tr.source,
        controller: tr.controller,
        ability: tr.ability,
        card: tr.card,
        face: tr.face,
        info: tr.info,
        x: if entering { tr.info.amount } else { 0 },
        ..StackEntry::default()
    })?;
    crate::cast::target_trigger(s, t, sink)
}

/// Creates a delayed triggered ability ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
pub(crate) fn delay(s: &mut State, trigger: PendingTrigger, at: Delayed) -> Result<(), Full> {
    s.delayed.push(crate::state::DelayedTrigger { trigger, at })
}
