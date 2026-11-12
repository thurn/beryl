//! Tokens ([CR 111](https://yawgatog.com/resources/magic-rules/#R111)).

use beryl_card::{Amount, CopyException, Objects, Players};

use crate::PlayerSet;
use crate::bitboard::Bitboard;
use crate::card::{CardDef, FaceIx, Part, Prompt, Purpose, Stop};
use crate::choices;
use crate::combat;
use crate::decision::Opt;
use crate::event::{self, Entry, Event, Reason};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::{Seat, SlotId};
use crate::interpret::{Outcome, Run, no_primitive};
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::resolution::Resolution;
use crate::state::{TARGETS, Zone};
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

/// Creates `count` tokens with the copiable values `what` under `controller`'s control,
/// tapped and attacking one of `defending`, or a planeswalker one of them controls. As they
/// enter, their controller chooses which
/// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a creature is put
/// onto the battlefield attacking, its controller chooses which defending player,
/// planeswalker a defending player controls, or battle a defending player protects it's
/// attacking as it enters the battlefield". Only the attacking player's creatures can be
/// ([CR 506.3b](https://yawgatog.com/resources/magic-rules/#R5063b)): "If an effect would
/// put a creature onto the battlefield attacking under the control of any player except an
/// attacking player, that creature does enter the battlefield, but it's never considered to
/// be an attacking creature." [CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062):
/// "During the combat phase, the active player is the attacking player". Nor can they attack
/// a player who is not a defending player
/// ([CR 506.3c](https://yawgatog.com/resources/magic-rules/#R5063c)): the creature "does
/// enter the battlefield, but it's never considered to be an attacking creature."
///
/// The declaration is the two-player game's ([`combat::attack_targets`]): of `defending`,
/// the one defending player there is. Returns the tokens.
pub fn create_attacking<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    controller: Seat,
    what: Copiable<L>,
    count: u8,
    defending: PlayerSet,
) -> Result<Bitboard<L::Slot>, Stop> {
    let s = &*game.state;
    let attacking = combat::in_combat(s.turn().step())
        && s.turn().active() == controller
        && defending.any(|player| s.defending_players().contains(player));
    let how = TokenEntry {
        tapped: true,
        attacking,
    };
    let tokens = create(game, controller, what, count, how)?;
    if attacking {
        // The choice is the controller's "as it enters", yet asked once the tokens are on
        // the battlefield, attacking the player, so they see what they choose for. Nothing
        // sees them in between: it is one step of the resolution, in which no player gets
        // priority and no ability triggers or state-based action is checked
        // ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c): "The
        // controller of the spell or ability follows its instructions in the order
        // written").
        let opts = combat::attack_targets(game.state)?;
        let aim = Prompt::new(Purpose::Attack).part(Part::Aim);
        let (opts, you) = (opts.as_slice(), controller);
        let Opt::Defender(walker) = choices::aim(game, resolution, you, tokens, opts, aim)? else {
            unreachable!("a defender")
        };
        if let Some(walker) = walker {
            // Each token is told what it attacks, with no event: the order is nothing.
            for slot in game.state.in_candidate_order(tokens) {
                game.state.choose_attacked(slot, Some(walker));
            }
        }
    }
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

/// [`Effect::CreateCopies`](beryl_card::Effect::CreateCopies): "[Player] creates [count]
/// tokens that are copies of [object]"
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "the copy acquires
/// the copiable values of the original object's characteristics". Those of the ability's
/// own source are its card's once it has left (CR 608.2h), as offspring's "a copy of it"
/// (CR 702.175a), and those of a target the effect has moved are its last known ones (CR
/// 608.2h): an exiled token has ceased to exist
/// ([CR 111.7](https://yawgatog.com/resources/magic-rules/#R1117)) and is copied all the
/// same. The outcome is the tokens.
pub(crate) fn create_copies_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    of: Objects,
    count: Amount,
    except: CopyException,
) -> Result<Outcome<L>, Full> {
    // CR 707.9b: "The final set of values for that characteristic becomes part of the
    // copiable values of the copy."
    let excepted = |what: Copiable<L>| match except {
        CopyException::None => what,
        CopyException::PowerToughness(1, 1) => what.except_one_one(),
        CopyException::PowerToughness(..) => no_primitive(&except),
    };
    let count = run.scene().amount(count);
    if count == 0 {
        return Ok(Outcome::NOTHING);
    }
    let who = run.scene().players(players);
    let mut created = Bitboard::EMPTY;
    let mut copy = |game: &mut Game<'_, L>, what: Copiable<L>| -> Result<(), Full> {
        for player in game.state.players_in_turn_order(who) {
            let how = TokenEntry::default();
            created |= create(game, player, excepted(what), count, how)?;
        }
        Ok(())
    };
    let mut originals = run.permanents(of);
    if originals.is_empty() && matches!(of, Objects::This) {
        let card = run.resolution().entry.card;
        copy(run.game(), Copiable::of_card(card))?;
    }
    while let Some(object) = run.next_permanent(&mut originals) {
        let what = run.state().copiable(object.slot);
        copy(run.game(), what)?;
    }
    // A target for each place at most, in the order they were chosen.
    let mut moved = [None; TARGETS];
    for (known, kept) in run.scene().moved(of).zip(&mut moved) {
        *kept = known.copiable;
    }
    for what in moved.into_iter().flatten() {
        copy(run.game(), what)?;
    }
    Ok(Outcome::acted_on(created))
}

/// [`Effect::CreateAttackingTokens`](beryl_card::Effect::CreateAttackingTokens): "[Player]
/// creates [count] [token] tokens that are tapped and attacking [players] or a planeswalker
/// they control" (CR 508.4): [`create_attacking`]. The outcome is the tokens.
pub(crate) fn create_attacking_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    token: &'static CardDef,
    count: Amount,
    attacking: Players,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    if count == 0 {
        return Ok(Outcome::NOTHING);
    }
    let defending = run.scene().players(attacking);
    let mut tokens = Bitboard::EMPTY;
    for player in run.in_turn_order(players) {
        let (game, resolution) = run.resolving();
        let what = predefined(game.table, token);
        tokens |= create_attacking(game, resolution, player, what, count, defending)?;
    }
    Ok(Outcome::acted_on(tokens))
}
