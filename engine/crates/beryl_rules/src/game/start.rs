//! Starting the game: deck slots, who plays first, and the London mulligan (see CR 103).
//!
//! Implements: CR 103.1, CR 103.5

use beryl_kernel::format::Mulligan;

use crate::decision::Answers;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::replacement::event::Sink;
use crate::scratch::Scratch;
use crate::state::{MulliganStatus, Pending, State, Zone};
use crate::table::GameTable;
use crate::turn;
use crate::zone::Place;
use beryl_kernel::reference::{CardIn, Hand};

/// Settles the starting player, then asks who reveals a companion
/// ([`super::companion::ask_first`]) or deals the opening hands.
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
    if super::companion::ask_first(s, t) {
        return Ok(());
    }
    deal(s, t, sink)
}

/// Deals each player's opening hand in turn order and opens the mulligans.
pub(crate) fn deal<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    for player in turn_order(s) {
        s.draw_n(sink, player, t.format().opening_hand())?;
    }
    s.ask(Pending::Mulligans);
    Ok(())
}

/// The players in turn order from the starting player
/// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "The game's default turn
/// order begins with the starting player and proceeds clockwise."
pub(crate) fn turn_order<L: Layout>(s: &State<L>) -> impl Iterator<Item = Seat> + Clone + use<L> {
    s.players_in_turn_order_from(s.turn().starting_player(), s.players())
}

/// Records a keep or mulligan declaration. A player who keeps after taking mulligans then
/// puts that many cards on the bottom ([`keep`]).
pub(crate) fn declare<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    player: Seat,
    mulligan: bool,
) -> Result<(), Full> {
    if mulligan {
        s.declare_mulligan(player);
    } else {
        keep(s, player);
    }
    finish_if_kept(s, t, x, a)
}

/// Keeps a player's hand, leaving them a card to put on the bottom for each mulligan taken.
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035) has a player who takes a
/// mulligan draw a new hand, "then [put] a number of those cards equal to the number of
/// times that player has taken a mulligan on the bottom of their library in any order", and
/// "Once a player chooses not to take a mulligan, the remaining cards become that player's
/// opening hand." Here the player looks at the whole hand and declares first, and puts cards on
/// the bottom only once they keep, as Arena and tournament play do. Nothing anyone can see
/// differs: a hand mulliganed again is shuffled away whole, bottomed cards included, and the
/// bottomed cards are hidden either way.
fn keep<L: Layout>(s: &mut State<L>, player: Seat) {
    // More cards than a byte counts are more than any number of mulligans.
    let hand = u8::try_from(s.hand_size(player)).unwrap_or(u8::MAX);
    s.keep_hand(player, s.player(player).mulligans().min(hand));
}

/// Puts one card from a player's hand on the bottom of their library after a mulligan.
pub(crate) fn put_on_bottom<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    player: Seat,
    card: CardIn<Hand, L>,
) -> Result<(), Full> {
    s.move_object(&mut x.events, card.slot(), Zone::Library, Place::Bottom)?;
    s.count_bottomed(player);
    finish_if_kept(s, t, x, a)
}

/// When both players have kept their hands, starts the first turn. After a restart, the
/// carried cards may ask replacement questions as they enter, using the step-action record.
fn finish_if_kept<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
) -> Result<(), Full> {
    if !advance_mulligans(s, t, &mut x.events)? {
        return Ok(());
    }
    if s.restarted() {
        return turn::paused_step::step_actions(s, t, x, a);
    }
    turn::begin_turn(s, t, &mut x.events)
}

/// Runs the London mulligan until a player must answer or every player has kept.
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
) -> Result<bool, Full> {
    let format = t.format();
    // The free mulligan of the shared-deck format is not offered yet: no shared-deck table
    // can be dealt until the kernel stores a shared library.
    let (Mulligan::London | Mulligan::LondonWithFreeRedraw) = format.mulligan();
    let opening_hand = format.opening_hand();
    let order = turn_order(s);
    loop {
        for p in order.clone() {
            let player = s.player(p);
            if player.mulligan() == MulliganStatus::Undeclared && player.mulligans() >= opening_hand
            {
                keep(s, p);
            }
        }
        if next_to_answer(s).is_some() {
            return Ok(false);
        }
        let mut any = false;
        for p in order.clone() {
            if s.player(p).mulligan() != MulliganStatus::WillMulligan {
                continue;
            }
            any = true;
            s.shuffle_opening_hand(sink, p)?;
            s.draw_n(sink, p, opening_hand)?;
            s.count_mulligan(p);
        }
        if !any {
            s.settle_question();
            return Ok(true);
        }
    }
}

/// Who answers the open mulligan question, and whether it is a bottom pick.
pub(crate) fn mulligan_question<L: Layout>(s: &State<L>) -> (Seat, bool) {
    next_to_answer(s).unwrap_or((s.turn().starting_player(), false))
}

/// The player in turn order who still has a mulligan question to answer, and whether it is a
/// bottom pick. Bottom picks come first: they finish the hand just kept.
fn next_to_answer<L: Layout>(s: &State<L>) -> Option<(Seat, bool)> {
    let mut order = turn_order(s);
    if let Some(p) = order.clone().find(|&p| s.player(p).to_bottom() > 0) {
        return Some((p, true));
    }
    order
        .find(|&p| s.player(p).mulligan() == MulliganStatus::Undeclared)
        .map(|p| (p, false))
}
