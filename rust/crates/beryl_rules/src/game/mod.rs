//! [`Game`]: what every function of the rules library works on (the crate's contract,
//! "Game").
//!
//! Implements: none. Groups the state, card tables and event sink for rules operations.

pub mod ending;
pub mod life;
pub(crate) mod start;

use crate::fixed::Full;
use crate::layout::{Layout, Small};
use crate::replacement::event::{self, Event, Sink};
use crate::state::State;
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

    /// Inserts a step in the remaining turn. The custom body's registration must set
    /// `changes_turn_structure`.
    pub fn insert_step(
        &mut self,
        anchor: beryl_card::TurnStep,
        placement: beryl_kernel::Placement,
        step: beryl_card::TurnStep,
    ) -> Result<(), Full> {
        if let Some(event) = self.state.insert_step(anchor, placement, step)? {
            self.sink.note(event);
        }
        Ok(())
    }

    /// Inserts a phase relative to the named phase.
    pub fn insert_phase(
        &mut self,
        anchor: beryl_kernel::TurnPhase,
        placement: beryl_kernel::Placement,
        phase: beryl_kernel::TurnPhase,
    ) -> Result<(), Full> {
        if let Some(event) = self.state.insert_phase(anchor, placement, phase)? {
            self.sink.note(event);
        }
        Ok(())
    }

    /// Skips the next scheduled step with this name.
    pub fn remove_step(&mut self, step: beryl_card::TurnStep) {
        if let Some(event) = self.state.remove_step(step) {
            self.sink.note(event);
        }
    }

    /// Skips the next scheduled phase of this kind.
    pub fn remove_phase(&mut self, phase: beryl_kernel::TurnPhase) {
        if let Some(event) = self.state.remove_phase(phase) {
            self.sink.note(event);
        }
    }

    /// Leaves only cleanup on the schedule. The caller carries out the other actions
    /// required when an effect ends the turn.
    pub fn cut_to_cleanup(&mut self) -> Result<(), Full> {
        let event = self.state.cut_to_cleanup()?;
        self.sink.note(event);
        Ok(())
    }
}
