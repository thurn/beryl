//! The timestamps of permanents that enter the battlefield together ([CR 613.7m]): "If two or
//! more objects would receive a timestamp simultaneously, such as by entering a zone
//! simultaneously or becoming attached simultaneously, their relative timestamps are
//! determined in APNAP order (see rule 101.4). Objects controlled by the active player (or
//! owned by the active player, if they have no controller) have an earlier relative
//! timestamp in the order of that player's choice, followed by each other player in turn
//! order."
//!
//! The kernel gives each permanent of a batch its timestamp as it moves, in the order the
//! batch moves them (`State::in_game_order`). After the batch, [`order`] puts them in APNAP
//! order of their controllers and keeps the order of the batch within each player's. Where
//! that order could change the layer results ([`layers::order_may_matter`]), it opens a
//! question, [`Pending::OrderEntered`]: each controller in turn order picks their permanents
//! one at a time, earliest first. Until it is answered the permanents keep the batch's
//! order.
//!
//! The question waits for the engine's next stop, as waiting triggers do, rather than
//! pausing the move: a batch can be part of a state-based action or of a leaves-the-
//! battlefield return, which have no resolution to pause. So the enters abilities that
//! trigger on the batch are matched with the batch's order, and a batch moved while another
//! question is open (as a cost being paid) keeps the batch's order without asking.
//!
//! [CR 613.7m]: https://yawgatog.com/resources/magic-rules/#R6137m

use crate::bitboard::Bitboard;
use crate::card::{Prompt, Purpose};
use crate::decision::{Decision, DecisionKind};
use crate::engine::collapse;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, Queue};
use crate::state::{Pending, State, Zone};
use crate::table::GameTable;

/// Orders the timestamps of `entered`, the permanents a batch put onto the battlefield in
/// the order it moved them, by their controllers in APNAP order, and opens the question of
/// their order for each controller with two or more whose order could matter. The layers
/// must be fresh, for the controllers; the caller refreshes them again after.
///
/// Out of line: most batches put one permanent onto the battlefield, or none.
#[inline(never)]
pub(crate) fn order<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    entered: &[L::Slot],
) -> Result<(), Full> {
    if entered.len() < 2 {
        return Ok(());
    }
    let mut ordered: L::Queue<L::Slot, 128> = Default::default();
    let mut asked = Bitboard::EMPTY;
    for player in s.players_in_turn_order(s.players()) {
        let first = ordered.len();
        for &slot in entered {
            if s.object(slot).zone() == Zone::Battlefield && s.controller(slot) == player {
                ordered.push(slot)?;
            }
        }
        let theirs = &ordered.as_slice()[first..];
        if theirs.len() > 1 && layers::order_may_matter(s, t, theirs) {
            asked |= theirs.iter().copied().collect::<Bitboard<_>>();
        }
    }
    let on_battlefield = |&&slot: &&L::Slot| s.object(slot).zone() == Zone::Battlefield;
    if ordered.iter().ne(entered.iter().filter(on_battlefield)) {
        s.order_timestamps(ordered.as_slice());
    }
    if !asked.is_empty() && matches!(s.pending(), Pending::None) {
        s.ask(Pending::OrderEntered { group: asked });
    }
    Ok(())
}

/// The question of [`Pending::OrderEntered`]: the first player in turn order with
/// permanents of `group` picks the one of them with the earliest timestamp left.
pub(crate) fn decision<L: Layout>(s: &State<L>, group: Bitboard<L::Slot>) -> Decision<L> {
    let (player, theirs) = next_to_order(s, group);
    Decision::objects(
        player,
        DecisionKind::ChooseObject,
        collapse(s, theirs),
        Prompt::new(Purpose::EnterOrder).most(1),
    )
}

/// Answers the question of [`Pending::OrderEntered`] with `picked`, which takes the
/// earliest of its controller's timestamps left in `group`. Once one permanent of theirs is
/// left, its timestamp is the latest, and the next player in turn order is asked, if any.
pub(crate) fn pick<L: Layout>(
    s: &mut State<L>,
    group: Bitboard<L::Slot>,
    picked: L::Slot,
) -> Result<(), Full> {
    let (_, theirs) = next_to_order(s, group);
    let mut order: L::Queue<L::Slot, 128> = Default::default();
    order.push(picked)?;
    for &slot in s.battlefield() {
        if slot != picked && theirs.contains(slot) {
            order.push(slot)?;
        }
    }
    order.as_mut_slice()[1..].sort_unstable_by_key(|&slot| s.object(slot).timestamp());
    s.order_timestamps(order.as_slice());
    let mut left = group;
    left.remove(picked);
    if (left & theirs).count() < 2 {
        left &= !theirs;
    }
    if left.is_empty() {
        s.settle_question();
    } else {
        s.ask(Pending::OrderEntered { group: left });
    }
    Ok(())
}

/// The first player in turn order who controls permanents of `group`, with those.
fn next_to_order<L: Layout>(s: &State<L>, group: Bitboard<L::Slot>) -> (Seat, Bitboard<L::Slot>) {
    s.players_in_turn_order(s.players())
        .map(|player| (player, group.filter(|slot| s.controller(slot) == player)))
        .find(|(_, theirs)| !theirs.is_empty())
        .expect("a question of the order of permanents with none left")
}
