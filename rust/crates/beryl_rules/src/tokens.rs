//! Tokens ([CR 111](https://yawgatog.com/resources/magic-rules/#R111)).

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::{CardDef, FaceIx};
use crate::event::{self, Entry, Event, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::{Seat, SlotId};
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::query::Copiable;
use crate::state::Zone;
use crate::table::GameTable;

/// How a token is created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenEntry {
    pub tapped: bool,
    /// Created attacking (CR 508.4): never declared, so no attack triggers.
    pub attacking: bool,
}

/// The copiable values of a predefined token
/// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)), which the game's table
/// holds as it holds a card.
///
/// # Panics
///
/// If the table lacks it: a card's tokens must be listed in its `tokens`.
pub fn predefined<L: Layout>(t: &GameTable, def: &'static CardDef) -> Copiable<L> {
    let card = t.index_of(def);
    Copiable::of_card(card.expect("a token missing from its card's tokens"))
}

/// Creates `count` tokens with the copiable values `what` under `controller`'s control: a
/// predefined token's ([`predefined`]), or those of what the tokens copy. Returns the tokens.
///
/// [CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111): "Some effects put tokens
/// onto the battlefield." [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The
/// player who creates a token is its owner."
/// [CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072): "the copy acquires the
/// copiable values of the original object's characteristics".
///
/// The tokens take free slots from the shared pool (architecture §4.2) and enter as one
/// event; then one `TokensCreated` event follows for "whenever you create one or more".
pub fn create<L: Layout>(
    game: &mut Game<'_, L>,
    controller: Seat,
    what: Copiable<L>,
    count: u8,
    how: TokenEntry,
) -> Result<Bitboard<L::Slot>, Full> {
    let (s, t) = (&mut *game.state, game.table);
    let mut tokens = Bitboard::EMPTY;
    let mut free = L::Slot::pool().filter(|&slot| s.object(slot).zone() == Zone::Nowhere);
    for _ in 0..count {
        let slot = free.next().ok_or(Full)?;
        tokens.insert(slot);
    }
    // A copy of a double-faced permanent enters with the same face up (CR 707.8a).
    let mut face = FaceIx::FRONT;
    // Each token is made in its own slot, with no event; they enter together below, in the
    // batch's order.
    for slot in s.in_candidate_order(tokens) {
        face = s.make_token(t, slot, controller, what);
    }
    let entry = Entry {
        tapped: how.tapped,
        attacking: how.attacking,
        face,
        ..Entry::default()
    };
    let (to, reason) = (Zone::Battlefield, Reason::Created);
    event::move_batch(s, t, game.sink, tokens, to, reason, entry)?;
    game.settle(Event::TokensCreated { controller, tokens })?;
    Ok(tokens)
}

/// [`Effect::CreateTokens`](beryl_card::Effect::CreateTokens): "[Player] creates [count]
/// [token] tokens" ([CR 701.7a](https://yawgatog.com/resources/magic-rules/#R7017a)): "To
/// create one or more tokens with certain characteristics, put the specified number of
/// tokens with the specified characteristics onto the battlefield." The outcome is the
/// tokens.
pub(crate) fn create_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    token: &'static CardDef,
    count: Amount,
) -> Result<Outcome<L>, Full> {
    let count = run.scene().amount(count);
    if count == 0 {
        return Ok(Outcome::NOTHING);
    }
    let mut tokens = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let game = run.game();
        let what = predefined(game.table, token);
        tokens |= create(game, player, what, count, TokenEntry::default())?;
    }
    Ok(Outcome::acted_on(tokens))
}
