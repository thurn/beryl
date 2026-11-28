//! The steps of casting a spell (CR 601.2a to 601.2i) and activating an ability (CR 602.2a), in
//! order: announce, targets, pay, and the spell becomes cast.
//!
//! Implements: CR 702.37c, CR 708.4

use beryl_kernel::reference::Graveyard;

use crate::bitboard::Bitboard;
use crate::card::{
    CastingIx, CastingOption, FaceIx, ObjectAbility, Part, Prompt, Purpose, TargetSpec,
};
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::keywords::flashback;
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::replacement::event::{self, Event, EventKind, Reason, Sink};
use crate::stack::trigger;
use crate::state::{
    AbilityRef, CastKind, CastStep, Casting, Count, Pending, StackEntry, StackItem, State, Target,
    Tracker, Zone,
};
use crate::table::{Capability, GameTable};

use super::convoke;
use super::costs::{spent_by_entry, total_cost, x_range_start};
use super::graveyard;
use super::mana_payment::{self, SacrificeCost};
use super::parts;
use super::spent;
use super::start;
use super::targets::{TargetSet, target_options};
use super::timing::activated;

/// Begins casting a spell: moves it to the stack and starts the machine.
///
/// [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose the casting of
/// a spell, a player first moves that card ... from where it is to the stack. It becomes the
/// topmost object on the stack. ... and that player becomes its controller."
///
/// See [CR 712.8b](https://yawgatog.com/resources/magic-rules/#R7128b): "A player casting a modal
/// double-faced card ... chooses which face they are casting before putting it onto the
/// stack." See [CR 716.3b]: "While on the stack as an Adventure, the spell has only its
/// alternative characteristics."
///
/// [CR 716.3b]: https://yawgatog.com/resources/magic-rules/#R7163b
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
pub(crate) fn begin_cast<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    option: Opt<L>,
    resolving: Option<bool>,
) -> Result<(), Full> {
    let Opt::Cast { slot, face, costs } = option else {
        unreachable!("a cast option");
    };
    // An immediate cast uses the resolving effect's permission (see CR 608.2g).
    // A cast for escape uses its own alternative cost (see CR 118.9a), so a flashback
    // permission the card also has does nothing to it.
    let permission = if resolving.is_none() && !costs.escapes() {
        start::permission(s, player, slot, face)
    } else {
        None
    };
    if cfg!(debug_assertions) && costs.escapes() {
        hit_escape(s, t, slot);
    }
    // CR 702.37c: "Put it onto the stack (as a face-down spell with the same characteristics)".
    // CR 708.4: "Objects that are cast face down are turned face down before they are put
    // onto the stack". The stack record carries only the public characteristics.
    let from = s.object(slot).zone();
    let down = (face == FaceIx::DOWN).then(|| s.who_knows(slot));
    // The player who casts a card from a shared library or graveyard takes it.
    event::move_one_by(s, t, sink, player, slot, Zone::Stack, Reason::Cast)?;
    let card = if let Some(knew) = down {
        if from == Zone::Hand {
            s.hide_choice_from_hand(slot);
            s.look(sink, player, Bitboard::single(slot));
        } else {
            for opponent in s
                .players()
                .filter(|p| p != player && !knew.contains(p))
                .in_seat_order()
            {
                s.forget(opponent, slot);
            }
        }
        t.face_down_copy::<L>()
    } else {
        s.definition(slot).record(t).0
    };
    let spell = s
        .spell(s.object_ref(slot))
        .expect("a card just put on the stack");
    s.become_spell(spell, face, player);
    if face == FaceIx::BACK && s.object(slot).is_token() {
        s.set_prepare_spell_owner(spell, player);
    }
    s.put_on_stack(
        sink,
        StackEntry::new(StackItem::Spell {
            object: spell.into(),
            controller: player,
            card,
            face,
            costs,
        }),
    )?;
    if let Some(permission) = permission {
        start::begin_on_spell(s, spell, permission)?;
    }
    let free = resolving.unwrap_or(false);
    let def_face = t.def(card).face(face);
    let kind = CastKind::Spell {
        free,
        resolving: resolving.is_some(),
    };
    let step = if x_range_start(def_face, costs, free).is_some() {
        CastStep::X
    } else {
        CastStep::TargetCount
    };
    let c = Casting {
        from,
        from_graveyard_permission: resolving.is_none()
            && permission.is_none()
            && !costs.escapes()
            && from == Zone::Graveyard
            && t.can(Capability::CastsFromGraveyard),
        ..Casting::new(player, kind, step)
    };
    advance(s, t, sink, c)
}

/// Notes that the static ability that gives the card in `slot` escape did what it says, as the
/// card is cast for it ([`crate::coverage`]).
#[allow(
    clippy::disallowed_methods,
    reason = "marks the escape ability covered, by its place on the granting permanent's face"
)]
fn hit_escape<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) {
    if let Some(card) = s.card_in(s.object_ref(slot), Graveyard)
        && let Some(reach) = s.escape_from(t, card)
    {
        let face = s.definition(reach.source).face(t);
        crate::coverage::hit(&face.abilities[usize::from(reach.ability)]);
    }
}

/// Begins activating an ability: puts it on the stack and starts the machine.
///
/// [CR 602.2a](https://yawgatog.com/resources/magic-rules/#R6022a): "That ability is created
/// on the stack as an object that's not a card. It becomes the topmost object on the stack.
/// ... Its controller is the player who activated the ability."
#[allow(
    clippy::disallowed_methods,
    reason = "reads an activated ability by its place on the face `activated` names"
)]
pub(crate) fn begin_activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let (source, index) = activated(s, t, slot, ability).expect("an ability offered");
    let a = source.face(t).ability(index);
    let (card, face) = source.record(t);
    if a.is_loyalty()
        && let Some(permanent) = s.permanent_at(slot)
    {
        s.track(t, Tracker::LoyaltyActivated, permanent);
    }
    s.put_on_stack(
        sink,
        StackEntry::new(StackItem::Activated {
            of: AbilityRef {
                source: Some(s.object_ref(slot)),
                card,
                face,
                ability: index,
                controller: player.into(),
            },
        }),
    )?;
    let step = if a.cost().mana.x > 0 {
        CastStep::X
    } else {
        CastStep::TargetCount
    };
    let c = Casting::new(player, CastKind::Activation, step);
    advance(s, t, sink, c)
}

/// The entry of the spell or ability being cast, the top of the stack. Callers borrow it
/// rather than copy it: casting asks for it at every step, and the derived `Clone` of the
/// generic [`StackEntry`] copies field by field.
pub(super) fn top<L: Layout>(s: &State<L>) -> &StackEntry<L> {
    s.stack().last().expect("the spell or ability being cast")
}

/// The target specs of the spell or ability being cast.
pub(super) fn specs<L: Layout>(t: &GameTable, entry: &StackEntry<L>) -> &'static [TargetSpec] {
    entry.ability_in(t).map_or(&[], |a| a.targets())
}

/// Moves the machine on to its next question, or finishes.
pub(super) fn advance<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
) -> Result<(), Full> {
    loop {
        match c.step {
            CastStep::X
            | CastStep::Plan
            | CastStep::Manual { .. }
            | CastStep::Objects(_)
            | CastStep::Outlet { .. } => {
                break;
            }
            CastStep::TargetCount | CastStep::Target { .. } => {
                let entry = top(s);
                let specs = specs(t, entry);
                let Some(spec) = specs.get(c.spec.index()) else {
                    if c.triggered() {
                        let event = ability_event(entry);
                        s.settle_question();
                        sink.push(event);
                        return if t.listens_anywhere().hears(EventKind::Targeted) {
                            targeted(s, t)
                        } else {
                            Ok(())
                        };
                    }
                    return begin_payment(s, t, sink, c);
                };
                if c.step == CastStep::TargetCount {
                    // CR 601.2c: "If the spell has a variable number of targets, the player
                    // announces how many targets they will choose before they announce those
                    // targets."
                    if spec.optional {
                        break;
                    }
                    c.step = CastStep::Target { left: spec.count };
                }
                if c.step != (CastStep::Target { left: 0 }) {
                    break;
                }
                c.spec = c.spec.next();
                c.step = CastStep::TargetCount;
            }
        }
    }
    s.ask(Pending::Casting(c));
    Ok(())
}

/// Turns to paying the total cost, once everything is announced: an ability's parts that are
/// not mana and then its mana, or a spell's mana, with any cards delve exiles for it, and
/// then its parts ([`parts::pay`] says why in that order).
///
/// [CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
/// total cost."
fn begin_payment<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
) -> Result<(), Full> {
    if c.kind == CastKind::Activation {
        return parts::pay(s, t, sink, c, 0);
    }
    // A game whose cards have no delve, convoke or improvise pays a spell's mana alone.
    if t.can(Capability::PaysWithObjects) {
        return parts::pay_substitutes(s, t, sink, c);
    }
    pay_mana(s, t, sink, c)
}

/// Pays the mana of the total cost by plan. At priority, the manual profile instead pays
/// one source at a time. A cast during resolution uses the solver even under that profile,
/// so it can pay within the enclosing transaction.
pub(super) fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(s, t, entry, &c);
    match s.profile().payment {
        Payment::Manual if !c.resolving() => {
            if cost.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual { remaining: cost };
            s.ask(Pending::Casting(c));
            Ok(())
        }
        payment @ (Payment::Manual | Payment::KeepMostPayable | Payment::Greedy) => {
            if !cost.is_zero() && super::outlets::offered(s, t, c.player) {
                return super::outlets::pay_mana(s, t, sink, c, cost, payment);
            }
            let plans = mana_payment::plans(s, t, sink, entry, cost, payment == Payment::Greedy)?;
            debug_assert!(!plans.is_empty(), "a spell offered is payable");
            if plans.len() > 1 {
                c.step = CastStep::Plan;
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            if let Some(plan) = plans.as_slice().first() {
                let paying = mana::Paying::entry(s, t, top(s));
                let exclude = spent_by_entry(t, top(s));
                let spent = mana::pay(s, t, sink, c.player, cost, exclude, plan, paying)?;
                spent::record(s, t, &c, spent)?;
            }
            after_mana(s, t, sink, c)
        }
    }
}

/// Mana production and spending have completed before a spell's caster chooses the objects
/// its other parts take.
/// [CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g): "Mana abilities must be
/// activated before costs are paid." CR 601.2h permits spending that mana before exiling
/// cards; it does not permit activating mana abilities after paying the exile cost. Keeping
/// the unfinished cast pending also keeps session cancellation valid.
pub(super) fn after_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
) -> Result<(), Full> {
    match c.kind {
        CastKind::Spell { .. } => parts::pay(s, t, sink, c, 0),
        CastKind::Activation => {
            parts::pay_removing_source(s, t, sink)?;
            finish(s, t, sink, c)
        }
        CastKind::Trigger => finish(s, t, sink, c),
    }
}

/// The spell becomes cast, or the ability activated.
///
/// [CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i): "Any abilities that
/// trigger when a spell is cast or put onto the stack trigger at this time. If the spell's
/// controller had priority before casting it, they get priority." see CR 608.2g: after a spell
/// cast during a resolution, "no player receives priority". The enclosing resolution
/// continues and gives priority when it finishes (see CR 117.3b).
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
pub(super) fn finish<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
) -> Result<(), Full> {
    s.settle_question();
    if !c.resolving() {
        s.give_priority(c.player);
    }
    // The spell is counted as it becomes cast, before its cast triggers anything (CR 601.2i),
    // so an ability that triggers on the cast and counts the spells cast this turn counts it.
    if let StackItem::Spell { controller, .. } = top(s).item {
        s.count(t, Count::SpellsCast, controller)?;
    }
    if c.from_graveyard_permission
        && let StackItem::Spell { card, face, .. } = top(s).item
    {
        let x = top(s).x;
        graveyard::use_permission(s, t, c.player, t.def(card).face(face), x)?;
    }
    let entry = top(s);
    if let StackItem::Spell {
        object: spell,
        controller,
        card,
        face,
        costs,
    } = entry.item
    {
        if face == FaceIx::DOWN
            && cfg!(debug_assertions)
            && let Some(def) = s.card_def(t, spell.slot)
        {
            for option in def
                .front()
                .casting
                .iter()
                .filter(|o| matches!(o, CastingOption::Morph(_)))
            {
                crate::coverage::hit(option);
            }
        }
        let characteristics = t.def(card).face(face);
        for (option, i) in characteristics.casting.iter().zip(0..) {
            if costs.contains(CastingIx::new(i)) {
                crate::coverage::hit(option);
            }
        }
        let cast = Event::Cast {
            spell,
            controller,
            targets: entry.targets.clone(),
            x: entry.x,
            costs,
            flashback: flashback::paid(s, spell),
            from: c.from,
        };
        if face == FaceIx::BACK
            && let Some(source) = s.prepare_source(spell.slot)
        {
            // See CR 723.3c: "That permanent loses the prepared designation at the time the
            // spell becomes cast".
            s.unprepare(source)?;
        }
        event::settle(s, t, sink, cast)?;
    } else {
        sink.push(ability_event(entry));
    }
    if t.listens_anywhere().hears(EventKind::Targeted) {
        targeted(s, t)?;
    }
    Ok(())
}

/// Matches the triggers of the objects the spell or ability on top of the stack targets, now
/// that its targets are chosen and it is cast, activated or put on the stack (see [CR 603.2e]):
/// "Some trigger events use the word 'becomes' ... These trigger only at the time the named
/// event happens". Each object once, however many of the targets it is. Asked only in a game
/// whose cards listen to such an event, so others pay one AND.
///
/// [CR 603.2e]: https://yawgatog.com/resources/magic-rules/#R6032e
#[cold]
#[inline(never)]
pub(crate) fn targeted<L: Layout>(s: &mut State<L>, t: &GameTable) -> Result<(), Full> {
    let entry = top(s);
    let controller = entry.controller();
    let spell = entry.spell();
    let objects: Bitboard<L::Slot> = (entry.targets.iter())
        .filter_map(|target| target.as_object())
        .filter(|&object| s.is_current(object))
        .map(|object| object.slot)
        .collect();
    for &slot in s.in_game_order(t, objects)?.iter() {
        let object = s.object_ref(slot);
        trigger::match_event(
            s,
            t,
            &Event::Targeted {
                object,
                controller,
                spell,
            },
        )?;
    }
    Ok(())
}

/// The number of other spells cast before the spell just cast this turn, by every player, which
/// a "when you cast" ability keeps as it triggers, for storm (see [CR 702.40a]): "copy it for each
/// other spell that was cast before it this turn." The spell itself was counted as it became
/// cast, before its Cast event. Zero in a game that keeps no count of spells cast.
///
/// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
pub(crate) fn cast_before<L: Layout>(s: &State<L>) -> Result<u8, Full> {
    use beryl_kernel::UnsignedNumber as _;
    let mut total: u8 = 0;
    for p in s.players().in_seat_order() {
        let count = u8::try_from(s.counted(Count::SpellsCast, p).to_u32()).map_err(|_| Full)?;
        total = total.checked_add(count).ok_or(Full)?;
    }
    Ok(if total == 0 { 0 } else { total - 1 })
}

/// The [`Event::Ability`] that says `entry`, an ability on the stack. It goes straight to the
/// sink: no trigger listens to it and it changes nothing.
pub(super) fn ability_event<L: Layout>(entry: &StackEntry<L>) -> Event<L> {
    Event::Ability {
        source: entry.object(),
        controller: entry.controller(),
        card: entry.card(),
        face: entry.face(),
        ability: entry.ability().expect("an ability's entry"),
        targets: entry.targets.clone(),
        x: entry.x,
    }
}

/// The X values the payment can still cover, from the least allowed, counting the creatures
/// and artifacts convoke and improvise may tap.
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face being cast (see CR 709.3b)"
)]
fn x_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let entry = top(s);
    let least = match entry.item {
        StackItem::Spell {
            card, face, costs, ..
        } => x_range_start(t.def(card).face(face), costs, c.free()).unwrap_or(0),
        StackItem::Activated { .. } | StackItem::Triggered { .. } => 0,
    };
    let exclude = spent_by_entry(t, entry);
    let paying = mana::Paying::entry(s, t, entry);
    let other = SacrificeCost::of_entry(t, entry);
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    // At most 64 candidate X values are offered per decision, capped at `u8::MAX`.
    for x in least..(least.saturating_add(64)) {
        if c.from_graveyard_permission
            && let StackItem::Spell { card, face, .. } = entry.item
            && graveyard::permitter(s, t, c.player, t.def(card).face(face), x).is_none()
        {
            break;
        }
        let mut probe = entry.clone();
        probe.x = x;
        let cost = total_cost(s, t, &probe, c);
        let available = match &other {
            Some(other) => other.payable(s, t, cost, paying)?,
            None => mana::payable(s, t, c.player, cost, exclude, paying)?,
        };
        let available = available
            || (t.can(Capability::PaysWithObjects) && convoke::entry_pays(s, t, c, cost)?);
        if !available {
            break;
        }
        out.push(Opt::Number(x))?;
    }
    Ok(out)
}

/// The open question of the machine.
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: &Casting<L>,
) -> Result<Decision<L>, Full> {
    let ask = |kind: DecisionKind, candidates: Candidates<L>, prompt: Prompt| Decision {
        player: c.player,
        decider: c.player,
        kind,
        candidates,
        prompt,
        about: Bitboard::EMPTY,
        filter: None,
    };
    let options = |kind, list, prompt| ask(kind, Candidates::Options(list), prompt);
    Ok(match c.step {
        CastStep::X => options(
            DecisionKind::X,
            x_options(s, t, c)?,
            Prompt::new(Purpose::X),
        ),
        CastStep::TargetCount => {
            let spec = &specs(t, top(s))[c.spec.index()];
            let most = u8::try_from(target_options(s, t, c).count())
                .unwrap_or(u8::MAX)
                .min(spec.count);
            let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
            for n in spec.minimum..=most {
                list.push(Opt::Number(n))?;
            }
            let count = Prompt::new(Purpose::Target)
                .most(spec.count)
                .part(Part::Count);
            options(DecisionKind::TargetCount, list, count)
        }
        // The targets of one spec are one prompt, which says how many are still to choose.
        // A spec that can choose players lists them as options, by seat, which a `Choice`
        // counts in, and then any objects it can also choose, in the candidate order. One
        // that chooses abilities on the stack lists them as options too.
        CastStep::Target { left } => ask(
            DecisionKind::Target,
            target_candidates(s, t, c)?,
            Prompt::new(Purpose::Target).most(left),
        ),
        CastStep::Plan if super::outlets::offered(s, t, c.player) => {
            super::outlets::plan_decision(s, t, sink, c)?
        }
        CastStep::Plan => {
            let entry = top(s);
            let cost = total_cost(s, t, entry, c);
            let plans = mana_payment::plans(s, t, sink, entry, cost, false)?;
            let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
            for i in 0..plans.len() {
                list.push(Opt::Plan(index::<L>(i)))?;
            }
            let plan = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
            options(DecisionKind::PaymentPlan, list, plan)
        }
        CastStep::Manual { remaining } => options(
            DecisionKind::ManaPayment,
            mana_payment::options(s, t, top(s), remaining)?,
            Prompt::new(Purpose::Mana).cost(remaining),
        ),
        CastStep::Objects(_) => parts::decision(s, t, c)?,
        CastStep::Outlet {
            slot,
            ability,
            paid,
            remaining,
        } => super::outlets::outlet_decision(s, t, c, slot, ability, paid, remaining)?,
    })
}

fn target_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> Result<Candidates<L>, Full> {
    let TargetSet {
        objects,
        players,
        abilities,
    } = target_options(s, t, c);
    let down = face_down_targets(s, t, objects);
    if players.is_empty() && abilities == 0 && down.is_empty() {
        return Ok(Candidates::Objects(collapse(s, objects)));
    }
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for player in players.in_seat_order() {
        list.push(Opt::Player(player))?;
    }
    // Abilities on the stack, the top first, as the stack is read.
    let mut unlisted = abilities;
    while unlisted != 0 {
        let place = usize::try_from(unlisted.ilog2()).expect("a bit in a u64");
        unlisted &= !(1 << place);
        list.push(Opt::Ability(index::<L>(place)))?;
    }
    Candidates::<L>::extend_options(&mut list, collapse(s, objects & !down), |out, slot| {
        out.push(Opt::Object(slot))
    })?;
    if !down.is_empty() {
        ordered_face_down_targets(s, down, &mut list)?;
    }
    Ok(Candidates::Options(list))
}

/// Applies an answer to the machine's question: an option, or an object for a target.
pub(crate) fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
    option: Option<Opt<L>>,
    object: Option<L::Slot>,
) -> Result<(), Full> {
    match c.step {
        CastStep::X => {
            let Some(Opt::Number(x)) = option else {
                unreachable!("an X answer");
            };
            s.announce_x(x);
            c.step = CastStep::TargetCount;
        }
        CastStep::TargetCount => {
            let Some(Opt::Number(n)) = option else {
                unreachable!("a target count");
            };
            c.step = CastStep::Target { left: n };
        }
        CastStep::Target { left } => {
            let target = match (option, object) {
                (Some(Opt::Player(player)), _) => Target::player(c.spec, player),
                (Some(Opt::Ability(place)), _) => Target::ability(c.spec, place.into()),
                (Some(Opt::Object(slot)), _) | (_, Some(slot)) => {
                    Target::object(c.spec, s.object_ref(slot))
                }
                _ => unreachable!("a target"),
            };
            s.announce_target(target)?;
            c.step = CastStep::Target { left: left - 1 };
        }
        CastStep::Plan if !matches!(option, Some(Opt::Plan(_))) => {
            let option = option.expect("an answer to a payment");
            return super::outlets::take(s, t, sink, c, option, false);
        }
        CastStep::Plan => {
            let Some(Opt::Plan(i)) = option else {
                unreachable!("a payment plan");
            };
            let entry = top(s);
            let cost = total_cost(s, t, entry, &c);
            let paying = mana::Paying::entry(s, t, entry);
            let exclude = spent_by_entry(t, entry);
            let plans = mana_payment::plans(s, t, sink, entry, cost, false)?;
            let spent = mana::pay(
                s,
                t,
                sink,
                c.player,
                cost,
                exclude,
                &plans.as_slice()[Into::<usize>::into(i)],
                paying,
            )?;
            spent::record(s, t, &c, spent)?;
            return after_mana(s, t, sink, c);
        }
        CastStep::Objects(_) => return parts::perform(s, t, sink, c, option, object),
        CastStep::Outlet {
            slot,
            ability,
            paid,
            remaining,
        } => {
            return super::outlets::perform_outlet(
                s, t, sink, c, slot, ability, paid, remaining, option, object,
            );
        }
        CastStep::Manual { remaining } => {
            let option = option.expect("a manual payment");
            if let Opt::Activate { slot, ability } = option
                && super::outlets::is_outlet(s, t, slot, ability)
            {
                let remaining = Some(remaining);
                return super::outlets::begin_outlet(
                    s, t, sink, c, slot, ability, remaining, false,
                );
            }
            let paying = mana::Paying::entry(s, t, top(s));
            let (remaining, color) =
                mana::manual_pay(s, t, sink, c.player, remaining, option, paying)?;
            if let Some(color) = color {
                spent::record_one(s, t, &c, color)?;
            }
            if remaining.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual { remaining };
        }
    }
    advance(s, t, sink, c)
}

/// See [CR 708.6]: players distinguish face-down objects by "the order spells were cast"
/// and "the order that face-down permanents entered the battlefield". Keep those orders
/// in the targets so a slot's position never depends on the name on its hidden face.
///
/// [CR 708.6]: https://yawgatog.com/resources/magic-rules/#R7086
fn face_down_targets<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    objects: Bitboard<L::Slot>,
) -> Bitboard<L::Slot> {
    if t.can(Capability::Morph) {
        objects.filter(|slot| s.object(slot).face() == FaceIx::DOWN)
    } else {
        Bitboard::EMPTY
    }
}

#[cold]
fn ordered_face_down_targets<L: Layout>(
    s: &State<L>,
    down: Bitboard<L::Slot>,
    list: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    for &slot in s.battlefield().iter().filter(|&&slot| down.contains(slot)) {
        list.push(Opt::Object(slot))?;
    }
    for spell in s.stack().iter().filter_map(StackEntry::spell) {
        if down.contains(spell.slot) {
            list.push(Opt::Object(spell.slot))?;
        }
    }
    Ok(())
}
