//! The pointer on the table: the card it hovers, picks up and drags (docs/beryl-3d.md, "Motion
//! the pointer drives"). Not built yet: it holds nothing and changes no card.

use crate::table::Look;

/// What the pointer is doing on the table.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointer {}

/// Changes `look`, how the card in `slot` is drawn, for what `pointer` is doing with it.
pub fn look(_pointer: &Pointer, _slot: usize, _look: &mut Look) {}
