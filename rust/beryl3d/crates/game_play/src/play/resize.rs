//! Placing the table again: for a window of a new shape, or while the opening holds the
//! viewer's hand below the window. Every script not yet ended is compiled again for the new
//! resting places.

use std::rc::Rc;
use std::sync::Arc;

use beryl_protocol::script::Table as Played;

use super::Show;
use super::playing::timings_of;
use game_table::table::Window;
use kit_base::data::Data;

impl Show {
    /// Replaces the tuning after finishing any script currently being compiled with it.
    /// Call `resize` next to lay out the table and compile the outstanding scripts again.
    pub fn set_data(&mut self, data: Arc<Data>) {
        self.complete();
        // Runs can outlast their scripts. Resolve those already on the show's clock against
        // the replacement tuning as well, keeping their anchors and times.
        self.effects.retain_mut(|run| {
            let Some(definition) = data.effects.get(&run.definition.name) else {
                return false;
            };
            run.definition = Arc::clone(definition);
            true
        });
        self.table.set_data(data);
    }

    /// Holds the viewer's hand below the window while the opening shows it, or brings it back
    /// (`Table::set_hand_away`), placing every card again and compiling every script not yet
    /// ended again for it, as a new window does. Does nothing if the hand is already held so,
    /// and returns whether it placed the table again.
    pub fn hold_hand_away(&mut self, away: bool, window: Window) -> bool {
        if self.table.hand_away() == away {
            return false;
        }
        self.table.set_hand_away(away);
        self.resize(window);
        true
    }

    /// Fits the table to a window of a new shape, and compiles every script not yet ended again
    /// for it, whole. A waiting script whose compiling had not begun is played from the table
    /// the script before it ends with, compiled again.
    pub fn resize(&mut self, window: Window) {
        self.complete();
        let shown = self.shown.as_ref().map(|update| &update.view);
        self.table.resize(window, shown);
        let mut before = shown;
        let mut ended: Option<Rc<Played>> = None;
        let mut compiled = Vec::new();
        if let Some(playing) = &self.playing
            && let Some(update) = &playing.update
        {
            let (again, played) = self.compile(
                playing.number,
                before,
                playing.from.as_deref(),
                update,
                timings_of(update, playing.timings.as_deref()),
            );
            compiled.push((playing.number, again, None));
            ended = Some(Rc::new(played));
            before = Some(&update.view);
        }
        for waiting in &self.waiting {
            let from = if waiting.pending {
                ended.clone().or_else(|| waiting.from.clone())
            } else {
                waiting.from.clone()
            };
            let (again, played) = self.compile(
                waiting.number,
                before,
                from.as_deref(),
                &waiting.update,
                timings_of(&waiting.update, waiting.timings.as_deref()),
            );
            compiled.push((waiting.number, again, from));
            ended = Some(Rc::new(played));
            before = Some(&waiting.update.view);
        }
        let pending = self.waiting.iter().any(|waiting| waiting.pending);
        for (number, again, from) in compiled {
            self.write_record(number, &again);
            if let Some(waiting) = self.waiting.iter_mut().find(|w| w.number == number) {
                if waiting.pending {
                    waiting.pending = false;
                    waiting.from = from;
                    self.unplaced += again.unplaced;
                }
                waiting.compiled = again;
            } else {
                self.recompiled = Some(again);
            }
        }
        // The next script to arrive is played from the table the last one ends with.
        if pending && let Some(ended) = ended {
            self.played = Some(Rc::unwrap_or_clone(ended));
        }
    }
}
