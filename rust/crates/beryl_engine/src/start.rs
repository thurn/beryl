//! Starting the game: deck slots, who plays first, and the London mulligan (CR 103).

use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{MulliganStatus, Pending, State, Zone};
use crate::table::GameTable;
use crate::turn;
use crate::zone::Place;

/// The starting hand size ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)),
/// "normally seven".
const STARTING_HAND: u8 = 7;

/// Settles the starting player, then deals opening hands.
///
/// [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031): "The player chosen to take
/// the first turn is the starting player."
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035): "Each player draws a number
/// of cards equal to their starting hand size, which is normally seven."
pub(crate) fn set_starting_player<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    starting: Seat,
) -> Result<(), Full> {
    s.set_starting_player(starting);
    for player in [starting, starting.opponent()] {
        s.draw_n(sink, player, STARTING_HAND)?;
    }
    s.ask(Pending::Mulligans);
    advance_mulligans(s, t, sink)
}

/// The players in turn order from the starting player.
pub(crate) fn turn_order<L: Layout>(s: &State<L>) -> [Seat; 2] {
    [
        s.turn().starting_player(),
        s.turn().starting_player().opponent(),
    ]
}

/// Records a keep or mulligan declaration. A player who keeps after taking mulligans then
/// puts that many cards on the bottom ([`keep`]).
pub(crate) fn declare<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    mulligan: bool,
) -> Result<(), Full> {
    if mulligan {
        s.declare_mulligan(player);
    } else {
        keep(s, player);
    }
    advance_mulligans(s, t, sink)
}

/// Keeps a player's hand, leaving them a card to put on the bottom for each mulligan taken.
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035) has a player who takes a
/// mulligan draw a new hand, "then [put] a number of those cards equal to the number of
/// times that player has taken a mulligan on the bottom of their library in any order", and
/// "Once a player chooses not to take a mulligan, the remaining cards become that player's
/// opening hand." Here the player looks at all seven and declares first, and puts cards on
/// the bottom only once they keep, as Arena and tournament play do. Nothing anyone can see
/// differs: a hand mulliganed again is shuffled away whole, bottomed cards included, and the
/// bottomed cards are hidden either way.
fn keep<L: Layout>(s: &mut State<L>, player: Seat) {
    let hand = s.hand_size(player) as u8;
    s.keep_hand(player, s.player(player).mulligans().min(hand));
}

/// Puts one card from a player's hand on the bottom of their library after a mulligan.
pub(crate) fn put_on_bottom<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    s.move_object(sink, slot, Zone::Library, Place::Bottom)?;
    s.count_bottomed(player);
    advance_mulligans(s, t, sink)
}

/// Runs the London mulligan until a player must answer or every player has kept, then begins
/// the first turn.
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035): "First, the starting player
/// declares whether they will take a mulligan. Then each other player in turn order does the
/// same. Once each player has made a declaration, all players who decided to take mulligans do
/// so at the same time. To take a mulligan, a player shuffles the cards in their hand back
/// into their library, draws a new hand of cards equal to their starting hand size, then puts
/// a number of those cards equal to the number of times that player has taken a mulligan on
/// the bottom of their library in any order. ... This process is then repeated until no
/// player takes a mulligan. A player can take mulligans until their opening hand would be zero
/// cards, after which they may not take further mulligans."
fn advance_mulligans<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let order = turn_order(s);
    loop {
        for &p in &order {
            let player = s.player(p);
            if player.mulligan() == MulliganStatus::Undeclared
                && player.mulligans() >= STARTING_HAND
            {
                keep(s, p);
            }
        }
        // Bottom picks come first: they finish the hand just kept.
        if order.iter().any(|&p| s.player(p).to_bottom() > 0) {
            return Ok(());
        }
        if order
            .iter()
            .any(|&p| s.player(p).mulligan() == MulliganStatus::Undeclared)
        {
            return Ok(());
        }
        let mut any = false;
        for &p in &order {
            if s.player(p).mulligan() != MulliganStatus::WillMulligan {
                continue;
            }
            any = true;
            // The shuffle that follows loses the order they went in.
            for slot in s.in_candidate_order(turn::hand(s, p)) {
                s.move_object(sink, slot, Zone::Library, Place::Top)?;
            }
            s.shuffle_library(p);
            s.draw_n(sink, p, STARTING_HAND)?;
            s.count_mulligan(p);
        }
        if !any {
            s.settle_question();
            return turn::begin_turn(s, t, sink);
        }
    }
}

/// Who answers the open mulligan question, and whether it is a bottom pick.
pub(crate) fn mulligan_question<L: Layout>(s: &State<L>) -> (Seat, bool) {
    let order = turn_order(s);
    if let Some(&p) = order.iter().find(|&&p| s.player(p).to_bottom() > 0) {
        return (p, true);
    }
    let p = order
        .into_iter()
        .find(|&p| s.player(p).mulligan() == MulliganStatus::Undeclared)
        .unwrap_or(order[0]);
    (p, false)
}
