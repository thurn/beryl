//! Triggered abilities: matching events and state, and putting waiting triggers on the
//! stack in APNAP order (architecture §8.3).
//!
//! Implements: CR 107.3m, CR 603.2, CR 603.3, CR 603.4, CR 603.6a, CR 603.8, CR 603.10

use crate::bitboard::Bitboard;
use crate::card::{Ability, AbilityIx, CastingKind, FaceIx, Subject, Trigger, TriggerEvent, Words};
use crate::continuous::layers;
use crate::filter::{Evaluate, Filter, is_for};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::replacement::event::{Event, EventKind, Listens, Recipient};
use crate::state::TriggeredAbility;
use crate::state::{AbilityRef, ObjectRef, PendingTrigger, State, TriggerInfo, Zone, ZoneKey};
use crate::table::{Capability, GameTable};

mod cast_number;
mod other_zones;
mod step;
mod waiting;

pub(super) use step::begins;
pub(crate) use waiting::{next_to_stack, put_on_stack, waiting_or_on_stack};

/// The triggered ability a waiting trigger or stack entry names: one of its face's own, or one
/// of its grants that a copy exception gives ([`Copiable::given_abilities`]).
#[inline]
#[allow(
    clippy::disallowed_methods,
    reason = "reads a triggered ability by the card and face its trigger recorded"
)]
pub(crate) fn ability<L: Layout>(
    t: &GameTable,
    of: &AbilityRef<L, TriggeredAbility>,
) -> &'static Ability {
    of.ability.of_face(t.def(of.card).face(of.face))
}

/// The triggered ability `ability` of the face `face` of `card`, as the object in `slot` has it
/// now: that object is its source, and the object's controller controls it.
#[inline]
pub(crate) fn of_object<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    card: L::Card,
    face: FaceIx,
    ability: TriggeredAbility,
) -> AbilityRef<L, TriggeredAbility> {
    AbilityRef {
        source: Some(s.object_ref(slot)),
        card,
        face,
        ability,
        controller: s.controller(slot).into(),
    }
}

/// The objects outside the battlefield with a triggered ability that works where they are: the
/// cards in a graveyard, in exile or on the stack whose card the game's table says has one for
/// that zone ([`GameTable::works_away`]). A game none of whose cards has such an ability reads
/// nothing ([`GameTable::zones_away`]), and a game with one reads the zones' membership, which the
/// zone move keeps, and the table, so the state keeps no set of its own.
///
/// see [CR 113.6b](https://yawgatog.com/resources/magic-rules/#R1136b): "An ability that states which
/// zones it functions in functions only from those zones."
pub(crate) fn away<L: Layout>(s: &State<L>, t: &GameTable) -> Bitboard<L::Slot> {
    let zones = t.zones_away();
    if zones.is_empty() {
        return Bitboard::EMPTY;
    }
    let mut candidates = Bitboard::EMPTY;
    for zone in zones.iter() {
        candidates |= match zone {
            Zone::Stack => s.zone(ZoneKey::Stack),
            zone @ (Zone::Library
            | Zone::Hand
            | Zone::Battlefield
            | Zone::Graveyard
            | Zone::Exile
            | Zone::Nowhere
            | Zone::Merged) => s
                .players()
                .in_seat_order()
                .fold(Bitboard::EMPTY, |all, p| all | s.zone(ZoneKey::Of(p, zone))),
        };
    }
    candidates.filter(|slot| {
        let (card, _) = s.definition(slot).record(t);
        t.works_away(card).contains(s.object(slot).zone())
    })
}

/// Matches an event against listeners on the battlefield, in other zones ([`away`]), and on
/// emblems (see [CR 114.4]: "Abilities of emblems function in the command zone"), then checks
/// delayed triggers waiting for a step. For entries ([CR 603.6a]): "all permanents on the
/// battlefield (including the newcomers) are checked for any enters-the-battlefield triggers".
///
/// [`GameTable::listeners`] indexes triggers by event kind. With no listeners for that kind,
/// the whole check is an AND ([`GameTable::listens_any`]).
///
/// [CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032): "Whenever a game event or game
/// state matches a triggered ability's trigger event, that ability automatically triggers."
///
/// [CR 114.4]: https://yawgatog.com/resources/magic-rules/#R1144
/// [CR 603.6a]: https://yawgatog.com/resources/magic-rules/#R6036a
pub(crate) fn match_event<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    e: &Event<L>,
) -> Result<(), Full> {
    let Some(k) = e.kind() else {
        return Ok(());
    };
    let mask = Listens::event(k);
    if t.listens_anywhere().intersects(mask) {
        // Only permanents can have changed what listens while the layer results are stale.
        let listening = s.listening().map_or(t.listens_any(), |listening| {
            debug_assert_eq!(
                listening,
                layers::listening(s, t, s.layer_results()).0,
                "the battlefield's listeners, as the layer results found them"
            );
            listening
        });
        if listening.intersects(mask) {
            for i in 0..s.battlefield().len() {
                let slot = s.battlefield()[i];
                let what = s.definition(slot);
                if !what.may_listen(t).intersects(mask) {
                    continue;
                }
                let (card, face) = what.record(t);
                let of = of_object(s, slot, card, face, TriggeredAbility::default());
                for l in t.listeners(card, face, k) {
                    // see CR 613.1f: a permanent that lost the ability has no trigger to match.
                    if s.has_ability(slot, l.ability) {
                        let ability = TriggeredAbility::own(l.ability);
                        fire(s, t, l.trigger, AbilityRef { ability, ..of }, e)?;
                    }
                }
            }
        }
        // The table counts the kinds that copy exceptions' triggered abilities listen to among
        // those that work away from a face of the battlefield's definitions.
        if t.listens_away().intersects(mask) {
            other_zones::match_away(s, t, k, e)?;
            if t.can(Capability::GivesTriggers) || t.can(Capability::Merges) {
                other_zones::match_given(s, t, k, e)?;
            }
        }
        // Triggers wait in the order found, which the queue of them keeps: by seat.
        for player in s.players().in_seat_order() {
            for index in 0..s.player(player).emblems().len() {
                let card = s.player(player).emblems()[index];
                if !Copiable::<L>::of_card(card).may_listen(t).intersects(mask) {
                    continue;
                }
                for l in t.listeners(card, FaceIx::FRONT, k) {
                    let of = AbilityRef {
                        source: None,
                        card,
                        face: FaceIx::FRONT,
                        ability: TriggeredAbility::own(l.ability),
                        controller: player.into(),
                    };
                    fire(s, t, l.trigger, of, e)?;
                }
            }
        }
    }
    if let Event::StepBegins { step, active, .. } = *e {
        super::delayed_triggers::fire_step(s, t, step, active)?;
    }
    Ok(())
}

/// Records the ability `of`, whose trigger is `trigger`, each time it triggers on `e`
/// ([`fires`], [`times`]). Its source, if it has one, is the object the trigger reads as "this".
fn fire<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    trigger: &Trigger,
    of: AbilityRef<L, TriggeredAbility>,
    e: &Event<L>,
) -> Result<(), Full> {
    let this = of.source.map(|source| source.slot);
    if let Some(info) = fires(s, t, trigger, this, of.controller.seat(), e) {
        for _ in 0..times(e) {
            enqueue(s, t, PendingTrigger { of, info })?;
        }
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
        Event::ZoneChange { .. }
        | Event::Damage { .. }
        | Event::DrawCard { .. }
        | Event::Scry { .. }
        | Event::TopOrBottom { .. }
        | Event::LibraryShuffled { .. }
        | Event::Reveal { .. }
        | Event::LifeGain { .. }
        | Event::LifeLoss { .. }
        | Event::CountersAdded { .. }
        | Event::CountersRemoved { .. }
        | Event::Tap { .. }
        | Event::Untap { .. }
        | Event::Mana { .. }
        | Event::ResolutionPayment { .. }
        | Event::AbilityFinished
        | Event::GameRestarted
        | Event::StackArrival { .. }
        | Event::StackDeparture { .. }
        | Event::TokensCreated { .. }
        | Event::Cast { .. }
        | Event::Ability { .. }
        | Event::Attack { .. }
        | Event::Block { .. }
        | Event::StepBegins { .. }
        | Event::TurnPlanChanged(_)
        | Event::GainControl { .. }
        | Event::CoinFlip { .. }
        | Event::Lose { .. }
        | Event::Targeted { .. }
        | Event::Win { .. }
        | Event::Mutated { .. }
        | Event::TurnedFaceUp { .. }
        | Event::ClassLevel { .. } => 1,
    }
}

/// A count of objects as the amount a trigger carries, which is a byte: past 255, which
/// only a layout of more slots than [`crate::Small`] can count, it is 255 and never the
/// count's low byte.
fn counted(count: u32) -> u8 {
    u8::try_from(count).unwrap_or(u8::MAX)
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
    };
    let whose = |player: Seat| is_for(&trigger.whose, player, you);
    let words = this.map_or(Words::NONE, |slot| s.words_of(slot));
    let about = |object: ObjectRef<L>| about(s, t, trigger.subject, this, you, &words, object);
    match (trigger.event, e) {
        (TriggerEvent::ClassLevel(wanted), &Event::ClassLevel { object, level })
            if wanted == level && about(object) =>
        {
            Some(info(Some(object), you, level))
        }
        // The amount is the X its spell was cast with
        // ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "If an object's
        // enters-the-battlefield triggered ability ... refers to X, and the spell that became
        // that object as it resolved had a value of X chosen for any of its costs, the value
        // of X for that ability is the same as the value of X for that spell".
        (event, &Event::ZoneChange { object, .. })
            if entered(s, event, object) && about(object) =>
        {
            Some(info(Some(object), you, s.x_of(object.slot)))
        }
        (TriggerEvent::Attacks, Event::Attack { attackers, .. })
            if this.is_some_and(|slot| attackers.contains(slot)) =>
        {
            Some(info(Some(s.object_ref(this?)), you, 0))
        }
        (TriggerEvent::PlayerAttacks, &Event::Attack { player, attackers }) if whose(player) => {
            let attacking = how_many(s, t, trigger.subject, attackers, you, &words);
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
        (TriggerEvent::DealsDamage, &Event::Damage { source, amount, .. }) if about(source) => {
            Some(info(Some(source), you, amount))
        }
        (TriggerEvent::TurnedFaceUp, &Event::TurnedFaceUp { object }) if about(object) => {
            Some(info(Some(object), you, 0))
        }
        (event, &Event::StepBegins { step, active, .. })
            if begins(event, step, || s.current_step_is_first()) =>
        {
            // See CR 303.4b: "The object or player an Aura is attached to is called enchanted."
            let matches = if trigger.whose == beryl_card::Players::Enchanted {
                this.and_then(|slot| s.object(slot).enchanted_player()) == Some(active)
            } else {
                whose(active)
            };
            matches.then(|| info(None, active, 0))
        }
        (TriggerEvent::CreatesTokens, &Event::TokensCreated { controller, tokens })
            if whose(controller) =>
        {
            let matching = how_many(s, t, trigger.subject, tokens, you, &words);
            (matching > 0).then(|| info(None, controller, counted(matching)))
        }
        (
            event,
            &Event::Cast {
                spell,
                controller,
                from,
                ..
            },
        ) if event.matches_cast_from(from) && whose(controller) && about(spell) => {
            cast_number::matches(s, controller, trigger.cast_number)
                .then(|| info(Some(spell), controller, crate::casting::cast_before(s)))
        }
        (TriggerEvent::Discards, &Event::ZoneChange { object, .. }) => {
            let discarder = s.object(object.slot).owner();
            (whose(discarder) && about(object)).then(|| info(Some(object), discarder, 0))
        }
        (TriggerEvent::Draws, &Event::Draw { player, .. }) if whose(player) => {
            Some(info(None, player, 0))
        }
        (
            event @ (TriggerEvent::BecomesTarget | TriggerEvent::BecomesTargetOfSpell),
            &Event::Targeted {
                object,
                controller,
                spell,
            },
        ) if whose(controller) && about(object) => {
            let subject = if matches!(event, TriggerEvent::BecomesTargetOfSpell) {
                spell?
            } else {
                object
            };
            Some(info(Some(subject), controller, 0))
        }
        (TriggerEvent::Mutates, &Event::Mutated { object, controller })
            if whose(controller) && about(object) =>
        {
            Some(info(Some(object), controller, 0))
        }
        _ => None,
    }
}

/// Whether an entering event, the move of `object` onto the battlefield, is what `event`
/// triggers on: "enters", or "enters untapped" if the permanent is untapped. Triggers are
/// matched as the move finishes, before anything else can tap or untap the permanent, so
/// whether it is tapped now is whether it entered tapped (CR 603.2).
fn entered<L: Layout>(s: &State<L>, event: TriggerEvent, object: ObjectRef<L>) -> bool {
    matches!(event, TriggerEvent::Enters)
        || matches!(event, TriggerEvent::EntersUntapped) && !s.object(object.slot).is_tapped()
}

/// Whether `object`, which an event is about, is a trigger's subject: the object its
/// ability is on, or one its filter accepts as it is now, read for the ability's controller
/// through `words`, those of the object the ability is on.
fn about<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: Subject,
    this: Option<L::Slot>,
    you: Seat,
    words: &Words,
    object: ObjectRef<L>,
) -> bool {
    match subject {
        Subject::This => this == Some(object.slot) && s.is_current(object),
        Subject::A(Filter::Any) => true,
        Subject::A(filter) => s.is_current(object) && filter.matches(s, t, object.slot, you, words),
    }
}

/// How many of `objects`, the set an event is about, a trigger's subject accepts: those on
/// the battlefield its filter accepts, read for the ability's controller through `words`, as
/// [`about`] reads it. "One or more" triggers once, with the number.
fn how_many<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    subject: Subject,
    objects: Bitboard<L::Slot>,
    you: Seat,
    words: &Words,
) -> u32 {
    let Subject::A(filter) = subject else {
        unreachable!("the card table refuses a set of objects that is this one");
    };
    (s.permanents_in(objects).slots())
        .filter(|slot| filter.matches(s, t, slot, you, words))
        .count()
}

/// Matches the abilities that look back in time against the permanents of one event that are
/// about to leave the battlefield, each with the zone it goes to ([CR 603.10a]): "Some zone-change
/// triggers look back in time. These are leaves-the-battlefield abilities". For those
/// ([CR 603.10]), "the game “looks back in time” to determine if those abilities trigger, using
/// the existence of those abilities and the appearance of objects immediately prior to the
/// event." Every permanent of the event is still on the battlefield, so each permanent's
/// abilities are matched against each leaving one as it is now, the permanent's own and those of
/// the others leaving with it: two creatures that die together each see the other die.
///
/// Each leaving permanent that some ability triggered on keeps its last-known information
/// ([`State::record_last_known`]) for the abilities to read once it has left.
///
/// A permanent dies as it goes to a graveyard, whatever its types (see [CR 700.4]): "The term dies
/// means 'is put into a graveyard from the battlefield.'" A filter such as "a creature" is read
/// against the layer results, so a permanent that is no longer a creature does not die as one.
///
/// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
/// [CR 603.10]: https://yawgatog.com/resources/magic-rules/#R60310
/// [CR 700.4]: https://yawgatog.com/resources/magic-rules/#R7004
pub(crate) fn match_look_back<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    leaving: impl Iterator<Item = (L::Slot, Zone)> + Clone,
) -> Result<(), Full> {
    if !s.delayed().is_empty() {
        crate::stack::delayed_triggers::fire_on_leaving(s, t, leaving.clone())?;
    }
    let mask = Listens::event(EventKind::Dies) | Listens::event(EventKind::Leaves);
    if !t.listens_anywhere().intersects(mask) {
        return Ok(());
    }
    if t.listens_away().intersects(mask) {
        other_zones::look_back_away(s, t, leaving.clone())?;
    }
    // In a game with mutate, a card under a merged permanent may listen where no permanent's
    // face does.
    let merges = t.can(Capability::Merges);
    if !t.listens_any().intersects(mask)
        || (!merges && !s.listening().unwrap_or(t.listens_any()).intersects(mask))
    {
        return Ok(());
    }
    for (left, to) in leaving {
        let object = s.object_ref(left);
        let waiting = s.triggers().len();
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            let what = s.definition(slot);
            let listens = what.may_listen(t);
            if !listens.intersects(mask) {
                continue;
            }
            let words = s.words_of(slot);
            let (card, face) = what.record(t);
            let of = of_object(s, slot, card, face, TriggeredAbility::default());
            for kind in look_back_kinds(to).filter(|&kind| listens.hears(kind)) {
                for l in t.listeners(card, face, kind) {
                    // CR 603.10: the game looks back to whether the ability existed just
                    // before the event, as the permanent is now.
                    if s.has_ability(slot, l.ability) {
                        let ability = TriggeredAbility::own(l.ability);
                        look_back(
                            s,
                            t,
                            l.trigger.subject,
                            AbilityRef { ability, ..of },
                            &words,
                            object,
                        )?;
                    }
                }
            }
        }
        if merges {
            other_zones::look_back_merged(s, t, left, to)?;
        }
        keep_last_known(s, object, waiting)?;
    }
    Ok(())
}

/// The kinds of event a permanent leaving the battlefield for `to` is part of, in the order
/// they are matched: it dies if it goes to a graveyard (see CR 700.4), and it leaves.
fn look_back_kinds(to: Zone) -> impl Iterator<Item = EventKind> {
    [
        (EventKind::Dies, to == Zone::Graveyard),
        (EventKind::Leaves, true),
    ]
    .into_iter()
    .filter_map(|(kind, happened)| happened.then_some(kind))
}

/// Records the ability `of`, which looks back in time, if `left`, a permanent about to leave
/// the battlefield, is its trigger's subject as it is now ([`about`]; see CR 603.10a). `words`
/// are those of its source. The trigger carries the permanent and the player who controls it.
fn look_back<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    subject: Subject,
    of: AbilityRef<L, TriggeredAbility>,
    words: &Words,
    left: ObjectRef<L>,
) -> Result<(), Full> {
    let this = of.source.map(|source| source.slot);
    if !about(s, t, subject, this, of.controller.seat(), words, left) {
        return Ok(());
    }
    let info = TriggerInfo {
        object: Some(left),
        player: s.controller(left.slot),
        amount: 0,
    };
    enqueue(s, t, PendingTrigger { of, info })
}

/// Keeps the last-known information of `left`, a permanent about to leave the battlefield, if
/// some ability triggered on it: if more than the `waiting` triggers from before it now wait.
fn keep_last_known<L: Layout>(
    s: &mut State<L>,
    left: ObjectRef<L>,
    waiting: usize,
) -> Result<(), Full> {
    if s.triggers().len() > waiting
        && let Some(permanent) = s.permanent(left)
    {
        s.record_last_known(permanent)?;
    }
    Ok(())
}

/// Triggers each state-triggered ability whose condition holds now and that is neither waiting to
/// be put on the stack nor on it ([CR 603.8]): "These abilities trigger as soon as the game state
/// matches the condition. ... A state-triggered ability doesn't trigger again until the ability has
/// resolved, has been countered, or has otherwise left the stack. Then, if the object with the
/// ability is still in the same zone and the game state still matches its trigger condition, the
/// ability will trigger again." Whether one is waiting or on the stack is found there, by its source
/// and ability, so nothing else records it; a source that changed zones is a new object, whose
/// ability is another.
///
/// Checked as state-based actions are, behind the game's [`EventKind::State`] listeners: a game
/// none of whose cards has a state trigger pays one AND, and the rest is out of line.
///
/// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
#[inline]
pub(crate) fn match_states<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    if t.listens_anywhere().hears(EventKind::State) {
        match_states_out_of_line(s, t)?;
    }
    Ok(())
}

#[cold]
#[inline(never)]
fn match_states_out_of_line<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    let kind = EventKind::State;
    if t.listens_any().hears(kind) && s.listening().unwrap_or(t.listens_any()).hears(kind) {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            let what = s.definition(slot);
            if !what.may_listen(t).hears(kind) {
                continue;
            }
            let (card, face) = what.record(t);
            for l in t.listeners(card, face, kind) {
                if s.has_ability(slot, l.ability) {
                    state_trigger(s, t, slot, card, face, l.ability, l.trigger)?;
                }
            }
        }
    }
    if t.listens_away().hears(kind) {
        for &slot in s.in_game_order(t, away(s, t))?.iter() {
            let zone = s.object(slot).zone();
            let (card, face) = s.definition(slot).record(t);
            for l in t.away_listeners(card, face, kind) {
                if l.zones.contains(zone) && s.has_ability(slot, l.ability) {
                    state_trigger(s, t, slot, card, face, l.ability, l.trigger)?;
                }
            }
        }
    }
    Ok(())
}

/// Triggers the state-triggered ability `ability` of the object in `slot`, if its condition
/// holds and it is neither waiting nor on the stack ([`match_states`]).
fn state_trigger<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    slot: L::Slot,
    card: L::Card,
    face: FaceIx,
    ability: AbilityIx,
    trigger: &Trigger,
) -> Result<(), Full> {
    let TriggerEvent::State(condition) = trigger.event else {
        unreachable!("a listener of state triggers is a state trigger");
    };
    let of = of_object(s, slot, card, face, TriggeredAbility::own(ability));
    let same = |other: &AbilityRef<L, TriggeredAbility>| {
        (other.source, other.card, other.face, other.ability)
            == (of.source, of.card, of.face, of.ability)
    };
    if waiting_or_on_stack(s, same) {
        return Ok(());
    }
    let info = TriggerInfo {
        object: None,
        player: of.controller.seat(),
        amount: 0,
    };
    let pending = PendingTrigger { of, info };
    if !crate::interpret::holds(s, t, &pending, condition) {
        return Ok(());
    }
    enqueue(s, t, pending)
}

/// Records a triggered ability, if its intervening "if" holds now ([CR 603.4]): "When the trigger
/// event occurs, the ability checks whether the stated condition is true. The ability triggers only
/// if it is; otherwise it does nothing."
///
/// [CR 603.4]: https://yawgatog.com/resources/magic-rules/#R6034
pub(crate) fn enqueue<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    pending: PendingTrigger<L>,
) -> Result<(), Full> {
    if !crate::interpret::condition_holds(s, t, &pending, ability(t, &pending.of)) {
        return Ok(());
    }
    s.queue_trigger(pending)
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
