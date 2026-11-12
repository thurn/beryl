//! Library ([CR 401](https://yawgatog.com/resources/magic-rules/#R401)).

use crate::bitboard::Bitboard;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::State;

/// The top `n` cards of a player's library, or all of it if it has fewer.
pub fn top<L: Layout>(s: &State<L>, player: Seat, n: u8) -> Bitboard<L::Slot> {
    let library = s.library(player);
    library[library.len().saturating_sub(usize::from(n))..]
        .iter()
        .copied()
        .collect()
}
