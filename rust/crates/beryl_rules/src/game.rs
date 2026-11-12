//! [`Game`]: what every function of the rules library works on (the crate's contract,
//! "Game").

use crate::event::{self, Event, Sink};
use crate::fixed::Full;
use crate::layout::{Layout, Small};
use crate::state::{ObjectRef, State, Zone};
use crate::table::GameTable;

/// A game as the rules act on it: the state, the tables its cards are read from, and the
/// sink that hears what happens. The three always travel together, so a library function
/// takes them as one.
///
/// A function that changes the game takes `&mut Game`. One that only reads takes the
/// [`State`] and the [`GameTable`], as the kernel's queries do, and a caller holding a game
/// lends them with [`Game::state`] and [`Game::table`]. A card's body gets the state to read
/// and no other way to it: it changes the game through the library alone.
pub struct Game<'a, L: Layout = Small> {
    pub(crate) state: &'a mut State<L>,
    pub(crate) table: &'a GameTable,
    pub(crate) sink: &'a mut Sink<L>,
}

impl<'a, L: Layout> Game<'a, L> {
    /// The game of a state, its tables and its sink, for code of the engine that still holds
    /// the three apart.
    #[inline]
    pub(crate) fn new(
        state: &'a mut State<L>,
        table: &'a GameTable,
        sink: &'a mut Sink<L>,
    ) -> Self {
        Game { state, table, sink }
    }

    /// The state as it is now, to read.
    #[inline]
    pub fn state(&self) -> &State<L> {
        self.state
    }

    #[inline]
    pub fn table(&self) -> &GameTable {
        self.table
    }

    /// Sends the event of a change a mutator made through the pipeline (architecture §8.2),
    /// so triggers, replacement effects, the layers and the sink see it.
    #[inline]
    pub(crate) fn settle(&mut self, e: Event<L>) -> Result<(), Full> {
        event::settle(self.state, self.table, self.sink, e)
    }

    /// Whether `object` is a permanent still: the object it was, on the battlefield
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that moves
    /// from one zone to another becomes a new object with no memory of, or relation to, its
    /// previous existence."
    #[inline]
    pub(crate) fn on_battlefield(&self, object: ObjectRef<L>) -> bool {
        self.state.is_current(object) && self.state.object(object.slot).zone() == Zone::Battlefield
    }
}
