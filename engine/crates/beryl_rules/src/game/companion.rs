//! Companions: revealing one before the game begins, and the special action that puts it into
//! its owner's hand for {3}.
//!
//! A player's sideboard is their cards outside the game, kept by the kernel
//! ([`State::sideboard`]). Before the opening hands are drawn, each player whose deck fulfills
//! the condition of a sideboard card's companion ability is asked whether to reveal one, in
//! turn order from the starting player. The answer is public, as the sideboard list is. Once
//! during the game, at sorcery speed, the player may pay {3} to put the revealed card into
//! their hand. The card then stays in the game like any other card brought in from outside
//! it.
//!
//! Implements: CR 103.2b, CR 116.2g, CR 702.139a, CR 702.139c

use beryl_card::{ManaCost, Prompt, Purpose, mana};

use crate::actions::reveal;
use crate::bitboard::Bitboard;
use crate::decision::{Decision, DecisionKind, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::mana as payment;
use crate::objects::face_down;
use crate::profile::Payment;
use crate::replacement::event::{self, Reason, Sink};
use crate::state::{Pending, State, Zone};
use crate::table::{Capability, GameTable};

use super::start;

/// What the special action costs ([CR 116.2g]): "A player who has chosen a companion may pay
/// {3} to put that card from outside the game into their hand."
///
/// [CR 116.2g]: https://yawgatog.com/resources/magic-rules/#R1162g
const COST: ManaCost = mana("{3}");

/// Asks the first player in turn order from the starting player who may reveal a companion,
/// and returns whether one was asked. A game whose cards have no companion ability, a
/// restarted game and a subgame ask no one.
///
/// [CR 103.2b](https://yawgatog.com/resources/magic-rules/#R1032b): "If any players wish to
/// reveal a card with a companion ability that they own from outside the game, they may do
/// so. A player may reveal no more than one card this way, and they may do so only if their
/// deck fulfills the condition of that card’s companion ability. The revealed card remains
/// outside the game."
pub(crate) fn ask_first<L: Layout>(s: &mut State<L>, t: &GameTable) -> bool {
    if !t.can(Capability::Companion) || s.restarted() {
        return false;
    }
    next_chooser(s, t, None).is_some_and(|chooser| {
        s.ask(Pending::Companion { chooser });
        true
    })
}

/// The next player in turn order after `after`, or from the starting player for none, who
/// may reveal a companion.
fn next_chooser<L: Layout>(s: &State<L>, t: &GameTable, after: Option<Seat>) -> Option<Seat> {
    let order = start::turn_order(s);
    let rest: Vec<Seat> = match after {
        Some(previous) => order.skip_while(|&p| p != previous).skip(1).collect(),
        None => order.collect(),
    };
    rest.into_iter()
        .find(|&p| s.companion_choices(t, p).next().is_some())
}

/// The question of whether `chooser` reveals a companion: each card they may reveal, by its
/// place in their sideboard list, and declining.
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    chooser: Seat,
) -> Result<Decision<L>, Full> {
    let mut options: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for (index, _) in s.companion_choices(t, chooser) {
        options.push(Opt::Number(index))?;
    }
    options.push(Opt::Done)?;
    Ok(Decision::options(
        chooser,
        DecisionKind::ChooseUpTo,
        options,
        Prompt::new(Purpose::Companion).most(1),
    ))
}

/// Records `chooser`'s answer, then asks the next player who may reveal one, or once none is
/// left deals the opening hands. A player who declines can't change their mind: no player
/// is asked twice.
pub(crate) fn answer<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    chooser: Seat,
    option: Option<Opt<L>>,
) -> Result<(), Full> {
    if let Some(Opt::Number(index)) = option {
        s.reveal_companion(chooser, index);
    }
    if let Some(next) = next_chooser(s, t, Some(chooser)) {
        s.ask(Pending::Companion { chooser: next });
        Ok(())
    } else {
        s.settle_question();
        start::deal(s, t, sink)
    }
}

/// Appends the companion special action to `player`'s priority options when they may take
/// it and pay for it.
///
/// [CR 702.139a](https://yawgatog.com/resources/magic-rules/#R702139a): "Once during the game,
/// any time you have priority and the stack is empty, but only during a main phase of your
/// turn, you may pay {3} and put that card into your hand."
pub(crate) fn options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    out: &mut L::Queue<Opt<L>, OPTIONS>,
) -> Result<(), Full> {
    if !t.can(Capability::Companion)
        || s.companion(t, player).is_none()
        || !crate::casting::sorcery_timing(s, player)
    {
        return Ok(());
    }
    let paying = payment::Paying::NOTHING;
    if payment::payable(s, t, player, COST, Bitboard::EMPTY, paying)? {
        out.push(Opt::Companion)?;
    }
    Ok(())
}

/// Takes the special action: pays {3} by the one payment plan there is, or asks how, then
/// puts the companion into its owner's hand.
///
/// See [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
pub(crate) fn begin<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
) -> Result<(), Full> {
    s.give_priority(player);
    if s.profile().payment != Payment::Manual {
        let greedy = s.profile().payment == Payment::Greedy;
        let plans = face_down::plans(s, t, sink, player, COST, greedy)?;
        debug_assert!(!plans.is_empty(), "the action is offered only when payable");
        if let [plan] = plans.as_slice() {
            face_down::pay_plan(s, t, sink, player, COST, plan)?;
            return put_into_hand(s, t, sink, player);
        }
    }
    s.ask(Pending::CompanionPayment {
        player,
        remaining: COST,
    });
    Ok(())
}

/// The question of paying for the special action.
pub(crate) fn payment_decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
) -> Result<Decision<L>, Full> {
    face_down::special_action_payment(s, t, sink, player, remaining)
}

/// Applies an answer to the question of paying for the special action, and puts the
/// companion into its owner's hand once the cost is paid.
pub(crate) fn pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    remaining: ManaCost,
    option: Opt<L>,
) -> Result<(), Full> {
    let remaining = face_down::pay_mana(s, t, sink, player, remaining, option)?;
    if remaining.is_zero() {
        s.settle_question();
        put_into_hand(s, t, sink, player)
    } else {
        s.ask(Pending::CompanionPayment { player, remaining });
        Ok(())
    }
}

/// Brings the revealed companion into the game and puts it into its owner's hand. It was
/// revealed before the game began, and is shown again as it comes in.
///
/// [CR 702.139c](https://yawgatog.com/resources/magic-rules/#R702139c): "Once you take the
/// special action and put the card with companion into your hand, it remains in the game until
/// the game ends."
fn put_into_hand<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
) -> Result<(), Full> {
    let (index, card) = s
        .companion(t, player)
        .expect("the action is offered only for a companion outside the game");
    // The companion ability did what it says: its card comes in from outside the game. The
    // action is taken once a game at most, so the search costs nothing worth a gate.
    if let Some(ability) = t
        .def(card)
        .front()
        .abilities
        .iter()
        .find(|ability| ability.companion_condition().is_some())
    {
        crate::coverage::hit(ability);
    }
    let slot = s
        .prepare_sideboard_card(t, player, index)?
        .expect("the companion is an available sideboard occurrence");
    let mut game = crate::game::Game::new(s, t, sink);
    reveal::reveal(&mut game, player, slot);
    event::move_one(s, t, sink, slot, Zone::Hand, Reason::Effect)
}
