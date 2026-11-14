//! The steps of casting a spell (CR 601.2a to 601.2i) and activating an ability (CR 602.2a), in
//! order: announce, targets, pay, and the spell becomes cast.

use crate::bitboard::Bitboard;
use crate::card::{CastingIx, ObjectAbility, Part, Prompt, Purpose, TargetSpec};
use crate::casting;
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Event, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::keywords::flashback;
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::state::{
    AbilityRef, CastKind, CastStep, Casting, Pending, StackEntry, StackItem, State, Target,
    Tracker, Zone,
};
use crate::table::GameTable;

use super::costs::{spent_by_entry, total_cost, x_range_start};
use super::parts;
use super::targets::{TargetSet, target_options};
use super::timing::{activated, entry_ability};

/// Begins casting a spell: moves it to the stack and starts the machine.
///
/// [CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "To propose the casting of
/// a spell, a player first moves that card ... from where it is to the stack. It becomes the
/// topmost object on the stack. ... and that player becomes its controller."
///
/// [CR 712.8b](https://yawgatog.com/resources/magic-rules/#R7128b): "A player casting a modal
/// double-faced card ... chooses which face they are casting before putting it onto the
/// stack."
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
    let permission = casting::permission(s, player, slot);
    event::move_one(s, t, sink, slot, Zone::Stack, Reason::Cast)?;
    let (card, _) = s.copiable(slot).record(t);
    s.become_spell(slot, face, player);
    s.put_on_stack(
        sink,
        StackEntry::new(StackItem::Spell {
            object: s.object_ref(slot),
            controller: player,
            card,
            face,
            costs,
        }),
    )?;
    if let Some(permission) = permission {
        casting::begin_on_spell(s, slot, permission)?;
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
    let c = Casting::new(player, kind, step);
    advance(s, t, sink, c)
}

/// Begins activating an ability: puts it on the stack and starts the machine.
///
/// [CR 602.2a](https://yawgatog.com/resources/magic-rules/#R6022a): "That ability is created
/// on the stack as an object that's not a card. It becomes the topmost object on the stack.
/// ... Its controller is the player who activated the ability."
pub(crate) fn begin_activate<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
    ability: ObjectAbility,
) -> Result<(), Full> {
    let (source, index) = activated(s, slot, ability).expect("an ability offered");
    let a = source.face(t).ability(index);
    let (card, face) = source.record(t);
    if a.is_loyalty() {
        s.track(t, Tracker::LoyaltyActivated, slot);
    }
    s.put_on_stack(
        sink,
        StackEntry::new(StackItem::Activated {
            of: AbilityRef {
                source: Some(s.object_ref(slot)),
                card,
                face,
                ability: index,
                controller: player,
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
    entry_ability(t, entry).map_or(&[], |a| a.targets())
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
            CastStep::X | CastStep::Plan | CastStep::Manual { .. } | CastStep::Objects(_) => {
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
                        return Ok(());
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
/// not mana and then its mana, or a spell's mana and then its parts ([`parts::pay`] says why
/// in that order).
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
    pay_mana(s, t, sink, c)
}

/// Pays the mana of the total cost: by plan, or one source at a time under manual payment.
pub(super) fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut c: Casting<L>,
) -> Result<(), Full> {
    let entry = top(s);
    let cost = total_cost(s, t, entry, &c);
    match s.profile().payment {
        Payment::Manual => {
            if cost.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual { remaining: cost };
            s.ask(Pending::Casting(c));
            Ok(())
        }
        payment => {
            let plans = mana::plans(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                payment == Payment::Greedy,
            )?;
            debug_assert!(!plans.is_empty(), "a spell offered is payable");
            if plans.len() > 1 {
                c.step = CastStep::Plan;
                s.ask(Pending::Casting(c));
                return Ok(());
            }
            if let Some(plan) = plans.as_slice().first() {
                mana::pay(s, t, sink, c.player, cost, Bitboard::EMPTY, plan)?;
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
fn after_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
) -> Result<(), Full> {
    if let CastKind::Spell { .. } = c.kind {
        return parts::pay(s, t, sink, c, 0);
    }
    finish(s, t, sink, c)
}

/// The spell becomes cast, or the ability activated.
///
/// [CR 601.2i](https://yawgatog.com/resources/magic-rules/#R6012i): "Any abilities that
/// trigger when a spell is cast or put onto the stack trigger at this time. If the spell's
/// controller had priority before casting it, they get priority." CR 608.2g: after a spell
/// cast during a resolution, "no player receives priority"; the resolution it was cast in is
/// over by now, so the active player receives priority (CR 117.3b).
pub(super) fn finish<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    c: Casting<L>,
) -> Result<(), Full> {
    s.settle_question();
    let active = s.turn().active();
    s.give_priority(if c.resolving() { active } else { c.player });
    let entry = top(s);
    if let StackItem::Spell {
        object: spell,
        controller,
        card,
        face,
        costs,
    } = entry.item
    {
        let face = t.def(card).face(face);
        for (i, option) in face.casting.iter().enumerate() {
            if costs.contains(CastingIx::new(i as u8)) {
                crate::coverage::hit(option);
            }
        }
        event::settle(
            s,
            t,
            sink,
            Event::Cast {
                spell,
                controller,
                targets: entry.targets.clone(),
                x: entry.x,
                costs,
                flashback: flashback::paid(s, spell),
            },
        )?;
    } else {
        sink.push(ability_event(entry));
    }
    Ok(())
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

/// The X values the payment can still cover, from the least allowed.
fn x_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> Result<L::Queue<Opt<L>, 64>, Full> {
    let entry = top(s);
    let least = match entry.item {
        StackItem::Spell {
            card, face, costs, ..
        } => x_range_start(t.def(card).face(face), costs, c.free()).unwrap_or(0),
        _ => 0,
    };
    let exclude = spent_by_entry(t, entry);
    let mut out: L::Queue<Opt<L>, 64> = Default::default();
    for x in least..(least.saturating_add(64)) {
        let mut probe = entry.clone();
        probe.x = x;
        if !mana::payable(s, t, c.player, total_cost(s, t, &probe, c), exclude)? {
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
    let options = |kind: DecisionKind, list: L::Queue<Opt<L>, 64>, prompt: Prompt| Decision {
        player: c.player,
        kind,
        candidates: Candidates::Options(list),
        prompt,
        about: Bitboard::EMPTY,
    };
    Ok(match c.step {
        CastStep::X => options(
            DecisionKind::X,
            x_options(s, t, c)?,
            Prompt::new(Purpose::X),
        ),
        CastStep::TargetCount => {
            let entry = top(s);
            let spec = &specs(t, entry)[c.spec.index()];
            let most = target_options(s, t, c).count().min(u32::from(spec.count));
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for n in 0..=most {
                list.push(Opt::Number(n as u8))?;
            }
            let count = Prompt::new(Purpose::Target)
                .most(spec.count)
                .part(Part::Count);
            options(DecisionKind::TargetCount, list, count)
        }
        // The targets of one spec are one prompt, which says how many are still to choose.
        CastStep::Target { left } => match target_options(s, t, c) {
            TargetSet::Objects(b) => Decision {
                player: c.player,
                kind: DecisionKind::Target,
                candidates: Candidates::Objects(collapse(s, b)),
                prompt: Prompt::new(Purpose::Target).most(left),
                about: Bitboard::EMPTY,
            },
            TargetSet::Players(p) => {
                let mut list: L::Queue<Opt<L>, 64> = Default::default();
                // Candidates are listed by seat, which a `Choice` counts in.
                for player in s.players_in_seat_order(p) {
                    list.push(Opt::Player(player))?;
                }
                options(
                    DecisionKind::Target,
                    list,
                    Prompt::new(Purpose::Target).most(left),
                )
            }
        },
        CastStep::Plan => {
            let entry = top(s);
            let cost = total_cost(s, t, entry, c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            let mut list: L::Queue<Opt<L>, 64> = Default::default();
            for i in 0..plans.len() {
                list.push(Opt::Plan(index::<L>(i)))?;
            }
            let plan = Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan);
            options(DecisionKind::PaymentPlan, list, plan)
        }
        CastStep::Manual { remaining } => options(
            DecisionKind::ManaPayment,
            mana::manual_options(s, t, c.player, remaining, Bitboard::EMPTY)?,
            Prompt::new(Purpose::Mana).cost(remaining),
        ),
        CastStep::Objects(_) => parts::decision(s, t, c)?,
    })
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
                (_, Some(slot)) => Target::object(c.spec, s.object_ref(slot)),
                _ => unreachable!("a target"),
            };
            s.announce_target(target)?;
            c.step = CastStep::Target { left: left - 1 };
        }
        CastStep::Plan => {
            let Some(Opt::Plan(i)) = option else {
                unreachable!("a payment plan");
            };
            let entry = top(s);
            let cost = total_cost(s, t, entry, &c);
            let plans = mana::plans(s, t, sink, c.player, cost, Bitboard::EMPTY, false)?;
            mana::pay(
                s,
                t,
                sink,
                c.player,
                cost,
                Bitboard::EMPTY,
                &plans.as_slice()[Into::<usize>::into(i)],
            )?;
            return after_mana(s, t, sink, c);
        }
        CastStep::Objects(_) => return parts::perform(s, t, sink, c, option, object),
        CastStep::Manual { remaining } => {
            let option = option.expect("a manual payment");
            let remaining = mana::manual_pay(s, t, sink, c.player, remaining, option)?;
            if remaining.is_zero() {
                return after_mana(s, t, sink, c);
            }
            c.step = CastStep::Manual { remaining };
        }
    }
    advance(s, t, sink, c)
}
