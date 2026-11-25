//! Face-down permanents (see [CR 708](https://yawgatog.com/resources/magic-rules/#R708)): turning
//! a permanent face up for a morph or manifest cost, a special action (CR 116.2b). Manifesting a card
//! (see CR 701.40) is [`crate::actions::manifest`].
//!
//! A face-down permanent's face is [`FaceIx::DOWN`], whose characteristics are
//! [`crate::card::FACE_DOWN`]'s whichever card it is, so layers, triggers, filters and costs
//! read the 2/2 with no text through [`crate::card::CardDef::face`] like any face. What stays
//! hidden is its card: its controller knows it, the other player doesn't (architecture §13).
//!
//! Implements: CR 116.2b, CR 701.40b, CR 701.40c, CR 702.37e, CR 712.12a
//!
//! A face-down permanent an effect cast face down may also be turned face up by a replacement
//! effect of its own, as it would be used ([`before_use`]).

use beryl_kernel::reference::Permanent;

use crate::bitboard::Bitboard;
use crate::card::{
    CastingOption, Chosen, Cost, CostAction, CostPart, FaceIx, ManaCost, Measure, Part, Prompt,
    Purpose, Types,
};
use crate::casting::parts;
use crate::continuous::layers;
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::game::{Game, life};
use crate::ids::Seat;
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::replacement::event::{Event, Sink};
use crate::stack::trigger;
use crate::state::{FaceUpPayment, PayingObjects, Pending, State};
use crate::table::GameTable;

/// The manifest and morph costs that can turn this permanent face up. Paying the manifest
/// cost means ([CR 701.40b](https://yawgatog.com/resources/magic-rules/#R70140b)): "show all
/// players that the card representing that permanent is a creature card and what that card's
/// mana cost is, pay that cost, then turn the permanent face up. ... (If the card representing
/// that permanent isn't a creature card or it doesn't have a mana cost, it can't be turned
/// face up this way.)" The card's characteristics are its front face's
/// (see [CR 712.6a](https://yawgatog.com/resources/magic-rules/#R7126a)), which is the face it
/// turns up ([CR 712.12a](https://yawgatog.com/resources/magic-rules/#R71212a)): "If it's
/// turned face up, it will have its front face up." No implemented creature card costs {0},
/// so a cost of nothing is no mana cost.
fn face_up_costs<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    permanent: Permanent<L>,
) -> [Option<Cost>; 2] {
    let Some(def) = s.card_def(t, permanent.slot()) else {
        return [None, None];
    };
    // Only a card is turned face up: a token copy of a face-down permanent has no card
    // behind it (see CR 707.2 and see CR 708.2).
    let front = def.front();
    if front.types.intersects(Types::INSTANT.union(Types::SORCERY)) {
        return [None, None];
    }
    // CR 702.37e: "show all players what the permanent’s morph cost would be if it were
    // face up, pay that cost, then turn the permanent face up."
    let morph = front.casting.iter().find_map(|option| {
        if let CastingOption::Morph(cost) = option {
            Some(*cost)
        } else {
            None
        }
    });
    // CR 701.40c: "If a card with morph is manifested, its controller may turn that card
    // face up using either the procedure described in rule 702.37e ... or the procedure
    // described above to turn a manifested permanent face up."
    let manifest = (!s.morphed(permanent.slot())
        && front.types.contains(Types::CREATURE)
        && !front.cost.is_zero())
    .then(|| Cost::mana(front.cost.with_x(0)));
    [manifest, morph]
}

/// Appends the special actions turning a face-down permanent `player` controls face up, for
/// those they can pay for
/// ([CR 116.2b](https://yawgatog.com/resources/magic-rules/#R1162b)): "Turning a face-down
/// creature face up is a special action. A player can take this action any time they have
/// priority."
pub(crate) fn face_up_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    if !s.any_face_down() {
        return Ok(());
    }
    for &slot in s.battlefield() {
        if s.object(slot).face() != FaceIx::DOWN || s.controller(slot) != player {
            continue;
        }
        let permanent = s
            .permanent_at(slot)
            .expect("a slot of the battlefield list");
        if available_costs(s, t, player, permanent)?
            .iter()
            .any(Option::is_some)
        {
            out.push(Opt::TurnFaceUp(slot))?;
        }
    }
    Ok(())
}

fn available_costs<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    permanent: Permanent<L>,
) -> Result<[Option<Cost>; 2], Full> {
    let mut costs = face_up_costs(s, t, permanent);
    for option in &mut costs {
        if let Some(cost) = *option
            && (!other_parts_payable(s, t, player, permanent, cost)
                || !mana::payable(
                    s,
                    t,
                    player,
                    cost.mana.with_x(0),
                    Bitboard::EMPTY,
                    mana::Paying::NOTHING,
                )?)
        {
            *option = None;
        }
    }
    Ok(costs)
}

/// Begins turning the face-down permanent in `slot` face up: pays its mana cost by the one
/// payment plan there is, or asks how, then turns it face up.
///
/// See [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
pub(crate) fn begin_turning_face_up<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    s.give_priority(player);
    let permanent = (s.permanent_at(slot)).expect("a permanent offered to turn face up");
    match available_costs(s, t, player, permanent)? {
        [Some(cost), Some(morph)] => {
            s.ask(Pending::TurningFaceUp {
                slot,
                cost,
                payment: FaceUpPayment::ChooseCost { morph },
            });
            Ok(())
        }
        [Some(cost), None] | [None, Some(cost)] => begin_payment(s, t, sink, permanent, cost),
        [None, None] => unreachable!("a permanent offered to turn face up"),
    }
}

fn begin_payment<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    cost: Cost,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let player = s.controller(slot);
    let remaining = cost.mana.with_x(0);
    if remaining.is_zero() {
        return pay_other_parts(s, t, sink, permanent, cost, 0);
    }
    if s.profile().payment != Payment::Manual {
        let greedy = s.profile().payment == Payment::Greedy;
        let plans = plans(s, t, sink, player, remaining, greedy)?;
        debug_assert!(!plans.is_empty(), "a cost offered is payable");
        if let [plan] = plans.as_slice() {
            pay_plan(s, t, sink, player, remaining, plan)?;
            return pay_other_parts(s, t, sink, permanent, cost, 0);
        }
    }
    s.ask(Pending::TurningFaceUp {
        slot,
        cost,
        payment: FaceUpPayment::Mana(remaining),
    });
    Ok(())
}

/// The question of a permanent being turned face up: a payment plan, or under manual
/// payment the next source (§10).
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    cost: Cost,
    payment: FaceUpPayment<L>,
) -> Result<Decision<L>, Full> {
    let player = s.controller(slot);
    let remaining = match payment {
        FaceUpPayment::ChooseCost { .. } => {
            let mut options: L::Queue<Opt<L>, OPTIONS> = Default::default();
            options.push(Opt::Plan(index::<L>(0)))?;
            options.push(Opt::Plan(index::<L>(1)))?;
            return Ok(Decision {
                player,
                decider: player,
                kind: DecisionKind::PaymentPlan,
                candidates: Candidates::Options(options),
                prompt: Prompt::new(Purpose::Mana).part(Part::Plan),
                about: Bitboard::single(slot),
            });
        }
        FaceUpPayment::Mana(remaining) => remaining,
        FaceUpPayment::Objects(paying) => {
            let (action, chosen) = chosen_part(cost, paying.part);
            let objects = parts::candidates(s, t, player, slot, action, &chosen) & !paying.chosen;
            return Ok(Decision {
                player,
                decider: player,
                kind: DecisionKind::ChooseObject,
                candidates: Candidates::Objects(collapse(s, objects)),
                prompt: Prompt::new(parts::purpose(action, &chosen)).most(paying.left),
                about: Bitboard::EMPTY,
            });
        }
    };
    special_action_payment(s, t, sink, player, remaining)
}

/// The question of paying `remaining`, the mana cost of a special action: a payment plan, or
/// under manual payment the next source (§10).
pub(crate) fn special_action_payment<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
) -> Result<Decision<L>, Full> {
    let (kind, part, list) = if s.profile().payment == Payment::Manual {
        (
            DecisionKind::ManaPayment,
            Part::Pick,
            mana::manual_options(
                s,
                t,
                player,
                remaining,
                Bitboard::EMPTY,
                mana::Paying::NOTHING,
            )?,
        )
    } else {
        let plans = plans(s, t, sink, player, remaining, false)?;
        let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
        for i in 0..plans.len() {
            list.push(Opt::Plan(index::<L>(i)))?;
        }
        (DecisionKind::PaymentPlan, Part::Plan, list)
    };
    Ok(Decision {
        player,
        decider: player,
        kind,
        candidates: Candidates::Options(list),
        prompt: Prompt::new(Purpose::Mana).cost(remaining).part(part),
        about: Bitboard::EMPTY,
    })
}

/// Applies an answer to the question of a permanent being turned face up.
pub(crate) fn perform<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    cost: Cost,
    payment: FaceUpPayment<L>,
    option: Opt<L>,
) -> Result<(), Full> {
    // The question is answered in the state that asked it, where the permanent is.
    let permanent = (s.permanent_at(slot)).expect("the permanent being turned face up");
    let player = s.controller(slot);
    let remaining = match payment {
        FaceUpPayment::ChooseCost { morph } => {
            let Opt::Plan(i) = option else {
                unreachable!("the chosen way to turn face up");
            };
            let selected = if Into::<usize>::into(i) == 0 {
                cost
            } else {
                morph
            };
            return begin_payment(s, t, sink, permanent, selected);
        }
        FaceUpPayment::Mana(remaining) => remaining,
        FaceUpPayment::Objects(mut paying) => {
            let Opt::Object(object) = option else {
                unreachable!("a choice of object to pay with");
            };
            paying.chosen.insert(object);
            paying.left -= 1;
            if paying.left != 0 {
                s.ask(Pending::TurningFaceUp {
                    slot,
                    cost,
                    payment: FaceUpPayment::Objects(paying),
                });
                return Ok(());
            }
            let (action, chosen) = chosen_part(cost, paying.part);
            parts::pay_with_all(s, t, sink, action, &chosen, player, paying.chosen)?;
            // The permanent may have paid for itself.
            let Some(permanent) = s.permanent(permanent.into()) else {
                s.settle_question();
                return Ok(());
            };
            return pay_other_parts(s, t, sink, permanent, cost, paying.part + 1);
        }
    };
    let remaining = pay_mana(s, t, sink, player, remaining, option)?;
    if remaining.is_zero() {
        pay_other_parts(s, t, sink, permanent, cost, 0)
    } else {
        s.ask(Pending::TurningFaceUp {
            slot,
            cost,
            payment: FaceUpPayment::Mana(remaining),
        });
        Ok(())
    }
}

/// The payment plans for the special action's mana cost, `remaining`.
pub(crate) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
    greedy: bool,
) -> Result<L::Queue<mana::Plan<L>, 64>, Full> {
    mana::plans(
        s,
        t,
        sink,
        player,
        remaining,
        Bitboard::EMPTY,
        greedy,
        mana::Paying::NOTHING,
    )
}

pub(crate) fn pay_plan<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
    plan: &mana::Plan<L>,
) -> Result<(), Full> {
    mana::pay(
        s,
        t,
        sink,
        player,
        remaining,
        Bitboard::EMPTY,
        plan,
        mana::Paying::NOTHING,
    )?;
    Ok(())
}

/// The part of the cost that takes chosen objects, at `part`.
fn chosen_part(cost: Cost, part: u8) -> (CostAction, Chosen) {
    let CostPart::Chosen(action, chosen) = cost.parts()[usize::from(part)] else {
        unreachable!("this part takes chosen objects");
    };
    (action, chosen)
}

/// Pays a mana plan or takes one step of manual payment for the special action.
pub(crate) fn pay_mana<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
    option: Opt<L>,
) -> Result<ManaCost, Full> {
    Ok(match option {
        Opt::Plan(i) => {
            let plans = plans(s, t, sink, player, remaining, false)?;
            pay_plan(
                s,
                t,
                sink,
                player,
                remaining,
                &plans.as_slice()[Into::<usize>::into(i)],
            )?;
            ManaCost::NONE
        }
        option @ (Opt::Pass
        | Opt::PlayLand(_)
        | Opt::Play
        | Opt::Draw
        | Opt::Keep
        | Opt::Mulligan
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Attack { .. }
        | Opt::Defender(_)
        | Opt::Block(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Activate { .. }
        | Opt::Mana { .. }
        | Opt::Pool(_)
        | Opt::Number(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_)
        | Opt::Companion) => {
            mana::manual_pay(s, t, sink, player, remaining, option, mana::Paying::NOTHING)?
        }
    })
}

// See CR 118.3: "A player can't pay a cost without having the necessary resources to pay it fully."
fn other_parts_payable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    permanent: Permanent<L>,
    cost: Cost,
) -> bool {
    let slot = permanent.slot();
    cost.parts().iter().all(|part| match *part {
        CostPart::Life(amount) => life::can_pay(s, t, player, amount),
        CostPart::Chosen(action, chosen) => parts::payable(s, t, player, slot, action, &chosen),
        CostPart::This(_) | CostPart::Loyalty(_) | CostPart::OneOf(_) | CostPart::DiscardHand => {
            unreachable!("the card table refuses unsupported morph costs")
        }
    })
}

/// Pays the nonmana parts of the morph cost (CR 702.37e). The object remains face down
/// until the entire cost is paid, and the ability created by turning it up can then trigger.
fn pay_other_parts<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    permanent: Permanent<L>,
    cost: Cost,
    from: u8,
) -> Result<(), Full> {
    let slot = permanent.slot();
    let player = s.controller(slot);
    for (part, &part_cost) in (from..).zip(cost.parts().iter().skip(usize::from(from))) {
        match part_cost {
            CostPart::Life(amount) => {
                life::lose(&mut Game::new(s, t, sink), player, i16::from(amount))?;
            }
            CostPart::Chosen(_, chosen) => {
                let Measure::Count(count) = chosen.measure else {
                    unreachable!("the card table refuses unsupported morph costs");
                };
                if count > 0 {
                    let payment = FaceUpPayment::Objects(PayingObjects::start(part, count));
                    s.ask(Pending::TurningFaceUp {
                        slot,
                        cost,
                        payment,
                    });
                    return Ok(());
                }
            }
            CostPart::This(_)
            | CostPart::Loyalty(_)
            | CostPart::OneOf(_)
            | CostPart::DiscardHand => {
                unreachable!("the card table refuses unsupported morph costs")
            }
        }
    }
    s.settle_question();
    // A mana ability that paid the cost may have sacrificed it.
    if let Some(permanent) = s.permanent(permanent.into()) {
        s.turn_face_up(t, permanent);
        layers::refresh(s, t, sink)?;
        let turned = Event::TurnedFaceUp {
            object: permanent.into(),
        };
        trigger::match_event(s, t, &turned)?;
    }
    Ok(())
}

/// Turns the face-down permanent in `slot` face up first if it would now be used and remembers
/// that it is turned face up then ([`crate::state::Memo::TurnsUpWhenUsed`]): Illusionary
/// Mask's "If the creature that spell becomes as it resolves has not been turned face up and
/// would assign or deal damage, be dealt damage, or become tapped, instead it's turned face up
/// and assigns or deals damage, is dealt damage, or becomes tapped." The event that would use
/// it then happens to the permanent face up. The rules call this as the permanent would
/// become tapped, assign combat damage, or deal or be dealt damage.
///
/// Turning it face up uses no stack and can't be responded to, as the card's ruling says of a
/// replacement effect (see [CR 614.1a]): "Effects that use the word 'instead' are replacement
/// effects." It applies before any other replacement or prevention of the same event, which
/// all still apply to the permanent face up. One AND in a game where nothing is cast face
/// down.
///
/// [CR 614.1a]: https://yawgatog.com/resources/magic-rules/#R6141a
#[inline]
pub(crate) fn before_use<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
) -> Result<(), Full> {
    if !t.can(crate::table::Capability::Morph) {
        return Ok(());
    }
    turn_up_if_used(s, t, sink, slot)
}

#[cold]
#[inline(never)]
fn turn_up_if_used<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
) -> Result<(), Full> {
    if s.object(slot).face() != FaceIx::DOWN || !s.turns_up_when_used(slot) {
        return Ok(());
    }
    let Some(permanent) = s.permanent_at(slot) else {
        return Ok(());
    };
    s.turn_face_up(t, permanent);
    layers::refresh(s, t, sink)?;
    let turned = Event::TurnedFaceUp {
        object: permanent.into(),
    };
    trigger::match_event(s, t, &turned)
}
