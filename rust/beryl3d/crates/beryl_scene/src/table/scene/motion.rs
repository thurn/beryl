//! What moves on the table while a script plays (`Motion`), and when a card turns to the
//! record the script leads to (`Turn`).

use beryl_protocol::protocol::PlayerView;

use super::Objects;
use crate::motion::Track;

/// What moves while a script plays: a track for each object that moves, by slot, and for each
/// ability that moves, by number; the time on the script's clock; and the scene objects at
/// rest in the view the script leads to.
#[derive(Clone, Copy, Debug)]
pub struct Motion<'a> {
    pub tracks: &'a [Track],
    /// Sorted by number.
    pub ability_tracks: &'a [(u32, Track)],
    pub t: u64,
    pub(crate) finals: &'a Objects,
    pub view: &'a PlayerView,
    /// When cards show the record the script leads to, where a step says (`Turn`), sorted by
    /// time.
    pub turns: &'a [Turn],
}

/// The card in `slot` shows the record the script leads to from `at` on the script's clock,
/// and the record the table showed before the script until then, as the web stage's
/// `Entering::cover` has it (`ui/src/stage/enter.ts`): a creature that strikes, and what it
/// strikes, show the damage of the blow as it lands. A card no turn names shows the record the
/// script leads to while it moves, and the earlier one while it stands still. `step` is the
/// number of the step that wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    pub slot: usize,
    pub at: u64,
    pub step: usize,
}
