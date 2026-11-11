//! Face-down permanents ([CR 708](https://yawgatog.com/resources/magic-rules/#R708)):
//! manifesting a card (CR 701.40), and turning a manifested permanent face up, a special
//! action (CR 116.2b).
//!
//! A face-down permanent's face is [`FaceIx::DOWN`], whose characteristics are
//! [`crate::card::FACE_DOWN`]'s whichever card it is, so layers, triggers, filters and costs
//! read the 2/2 with no text through [`crate::card::CardDef::face`] like any face. What stays
//! hidden is its card: its controller knows it, the other player doesn't (architecture §13).

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, ManaCost, Part, Prompt, Purpose, Types};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::engine::collapse;
use crate::event::{self, Entry, Reason, Sink};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, index};
use crate::mana;
use crate::profile::Payment;
use crate::state::{Pending, State, Zone};
use crate::table::GameTable;

/// Manifests the card in `slot` for `player`
/// ([CR 701.40a](https://yawgatog.com/resources/magic-rules/#R70140a)): "To manifest a card,
/// turn it face down. It becomes a 2/2 face-down creature card with no text, no name, no
/// subtypes, and no mana cost. Put that card onto the battlefield face down."
///
/// It is turned face down before it enters
/// ([CR 708.3](https://yawgatog.com/resources/magic-rules/#R7083)): "Objects that are put onto
/// the battlefield face down are turned face down before they enter the battlefield, so the
/// permanent's enters-the-battlefield abilities won't trigger (if triggered) or have any effect
/// (if static)." So it enters with [`FaceIx::DOWN`] up, and its own replacement and enters
/// abilities never apply.
///
/// The battlefield is a public zone, but a face-down card isn't seen
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Public zones are zones in
/// which all players can see the cards' faces, except for those cards that some rule or effect
/// specifically allow to be face down." Its controller may look at it (CR 708.5); the other
/// player knows it only if they already did.
pub(crate) fn manifest<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    player: Seat,
) -> Result<(), Full> {
    let knew = Seat::BOTH.map(|p| s.knows(p, slot));
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        Zone::Battlefield,
        Reason::Effect,
        Entry {
            controller: Some(player),
            face: FaceIx::DOWN,
            ..Entry::default()
        },
    )?;
    for p in Seat::BOTH {
        if !knew[p.index()] && p != player {
            s.forget(p, slot);
        }
    }
    Ok(())
}

/// The mana cost that turns the manifested permanent in `slot` face up, if it can be turned
/// face up ([CR 701.40b](https://yawgatog.com/resources/magic-rules/#R70140b)): "show all
/// players that the card representing that permanent is a creature card and what that card's
/// mana cost is, pay that cost, then turn the permanent face up. ... (If the card representing
/// that permanent isn't a creature card or it doesn't have a mana cost, it can't be turned
/// face up this way.)" The card's characteristics are its front face's
/// ([CR 712.8a](https://yawgatog.com/resources/magic-rules/#R7128a)), which is the face it
/// turns up ([CR 712.15a](https://yawgatog.com/resources/magic-rules/#R71215a)): "If it's
/// turned face up, it will have its front face up." No creature card in the cube costs {0},
/// so a cost of nothing is no mana cost.
fn face_up_cost<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Option<ManaCost> {
    // Only a card is turned face up: a token copy of a face-down permanent has no card
    // behind it (CR 707.2, 708.2).
    let front = s.card_def(t, slot)?.front();
    (front.types.contains(Types::CREATURE) && !front.cost.is_zero()).then(|| front.cost.with_x(0))
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
    out: &mut L::Queue<Opt<L>, 64>,
) -> Result<(), Full> {
    let face_down: Bitboard<L::Slot> = s
        .battlefield()
        .iter()
        .copied()
        .filter(|&slot| s.object(slot).face == FaceIx::DOWN && s.controller(slot) == player)
        .collect();
    for slot in collapse(s, face_down).iter() {
        if let Some(cost) = face_up_cost(s, t, slot)
            && mana::payable(s, t, player, cost, Bitboard::EMPTY)?
        {
            out.push(Opt::TurnFaceUp(slot))?;
        }
    }
    Ok(())
}

/// Begins turning the face-down permanent in `slot` face up: pays its mana cost by the one
/// payment plan there is, or asks how, then turns it face up.
///
/// [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
pub(crate) fn begin_turning_face_up<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    s.give_priority(player);
    let cost = face_up_cost(s, t, slot).expect("a permanent offered to turn face up");
    if s.profile().payment != Payment::Manual {
        let greedy = s.profile().payment == Payment::Greedy;
        let plans = mana::plans(s, t, sink, player, cost, Bitboard::EMPTY, greedy)?;
        debug_assert!(!plans.is_empty(), "a cost offered is payable");
        if plans.len() == 1 {
            mana::pay(
                s,
                t,
                sink,
                player,
                cost,
                Bitboard::EMPTY,
                &plans.as_slice()[0],
            )?;
            s.turn_face_up(slot);
            return Ok(());
        }
    }
    s.ask(Pending::TurningFaceUp {
        slot,
        remaining: cost,
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
    remaining: ManaCost,
) -> Result<Decision<L>, Full> {
    let player = s.controller(slot);
    let (kind, part, list) = if s.profile().payment == Payment::Manual {
        (
            DecisionKind::ManaPayment,
            Part::Pick,
            mana::manual_options(s, t, player, remaining, Bitboard::EMPTY)?,
        )
    } else {
        let plans = mana::plans(s, t, sink, player, remaining, Bitboard::EMPTY, false)?;
        let mut list: L::Queue<Opt<L>, 64> = Default::default();
        for i in 0..plans.len() {
            list.push(Opt::Plan(index::<L>(i)))?;
        }
        (DecisionKind::PaymentPlan, Part::Plan, list)
    };
    Ok(Decision {
        player,
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
    remaining: ManaCost,
    option: Opt<L>,
) -> Result<(), Full> {
    let player = s.controller(slot);
    let remaining = match option {
        Opt::Plan(i) => {
            let plans = mana::plans(s, t, sink, player, remaining, Bitboard::EMPTY, false)?;
            let plan = &plans.as_slice()[Into::<usize>::into(i)];
            mana::pay(s, t, sink, player, remaining, Bitboard::EMPTY, plan)?;
            ManaCost::NONE
        }
        option => mana::manual_pay(s, t, sink, player, remaining, option)?,
    };
    if remaining.is_zero() {
        s.settle_question();
        s.turn_face_up(slot);
    } else {
        s.ask(Pending::TurningFaceUp { slot, remaining });
    }
    Ok(())
}
