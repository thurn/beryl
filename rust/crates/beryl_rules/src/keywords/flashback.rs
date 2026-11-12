//! Flashback ([CR 702.34](https://yawgatog.com/resources/magic-rules/#R70234)).

use crate::casting;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::{Duration, MoveReplacement, ObjectRef, Permission, State};

/// The card in `object`'s slot gains flashback until end of turn, its flashback cost
/// equal to its mana cost, for `player`
/// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
/// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
/// instant or sorcery spell by paying [cost] rather than paying its mana cost' and 'If the
/// flashback cost was paid, exile this card instead of putting it anywhere else any time
/// it would leave the stack.'" Only its owner can cast it from their graveyard, so
/// `player` is its owner.
pub fn grant<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &Resolution<'_, L>,
    object: ObjectRef<L>,
    player: Seat,
) -> Result<(), Full> {
    let terms = Permission {
        player,
        increase: 0,
        replacement: Some(MoveReplacement::Flashback),
    };
    casting::permit(game, resolution, object, terms, Duration::EndOfTurn)
}

/// Whether the spell was cast for a flashback cost: the replacement that paying one starts
/// is on it ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a): "If the
/// flashback cost was paid, exile this card instead of putting it anywhere else any time it
/// would leave the stack.").
pub fn paid<L: Layout>(s: &State<L>, spell: ObjectRef<L>) -> bool {
    s.move_replacements(spell)
        .any(|r| r == MoveReplacement::Flashback)
}
