//! Adding mana ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)).

use beryl_card::{Amount, Players};

use crate::bitboard::Bitboard;
use crate::card::{ManaColor, ManaSet};
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::state::ObjectRef;

/// Adds mana to a player's mana pool, as an effect of `source`
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
/// instructs a player to add mana, that mana goes into a player's mana pool." Nothing
/// triggers on adding mana, so it skips the event pipeline.
#[inline]
pub fn add<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    source: Option<ObjectRef<L>>,
    color: ManaColor,
    amount: u8,
) {
    let added = game.state.add_mana(player, source, color, amount);
    game.sink.note(added);
}

/// [`Effect::AddMana`](beryl_card::Effect::AddMana): "[Player] adds [mana]" (CR 106.3): "A
/// spell or ability that produces mana instructs a player to add that mana", of the kind its
/// activator chose among those it produces. The outcome is the mana added.
pub(crate) fn add_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    mana: ManaSet,
) -> Outcome<L> {
    let (source, color) = (run.resolution().entry.object(), run.resolution().mana);
    debug_assert!(mana.includes(color));
    let mut added = 0;
    for player in run.in_turn_order(players) {
        add(run.game(), player, source, color, mana.amount);
        added += u32::from(mana.amount);
    }
    Outcome::of(Bitboard::EMPTY, added)
}

/// [`Effect::AddManaOfKind`](beryl_card::Effect::AddManaOfKind): "[Player] adds [amount]
/// mana of [kind]" (CR 106.4), by a spell or an ability that is no mana ability, of the one
/// kind it names. The outcome is the mana added.
pub(crate) fn add_of_kind_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    players: Players,
    kind: ManaColor,
    amount: Amount,
) -> Outcome<L> {
    let (source, amount) = (run.resolution().entry.object(), run.scene().amount(amount));
    let mut added = 0;
    for player in run.in_turn_order(players) {
        add(run.game(), player, source, kind, amount);
        added += u32::from(amount);
    }
    Outcome::of(Bitboard::EMPTY, added)
}
