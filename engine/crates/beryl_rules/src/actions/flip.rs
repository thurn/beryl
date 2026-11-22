//! Permanents with flipped status (CR 710.4).
//!
//! Implements: CR 710.4

use beryl_card::definition::Shape;
use beryl_card::{FaceIx, Objects};

use crate::bitboard::Bitboard;
use crate::continuous::layers;
use crate::fixed::Full;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::state::Flag;

/// Sets flipped status once. [CR 710.4] says "Once a permanent is flipped, it’s
/// impossible for it to become unflipped. However, if a flipped permanent leaves
/// the battlefield, it retains no memory of its status."
///
/// [CR 710.4]: https://yawgatog.com/resources/magic-rules/#R7104
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
) -> Result<Outcome<L>, Full> {
    let objects = run.scene().objects(objects);
    let mut affected = Bitboard::EMPTY;
    for &slot in run.in_game_order(objects)?.iter() {
        let game = run.game();
        let Some(permanent) = game.state.permanent(game.state.object_ref(slot)) else {
            continue;
        };
        let object = game.state.object(slot);
        if object.flags().has(Flag::Flipped) {
            continue;
        }
        let use_alternative = object.face() != FaceIx::DOWN
            && game.state.copiable(slot).shape(game.table) == Shape::Flip;
        game.state.flip_permanent(permanent, use_alternative);
        affected.insert(slot);
    }
    if !affected.is_empty() {
        let game = run.game();
        layers::refresh(game.state, game.table, game.sink)?;
    }
    Ok(Outcome::acted_on(affected))
}
