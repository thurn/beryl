//! Prepares table layout and records the flat layer's screen transitions.

use super::Client;
use app_screens::screens::Screen;
use game_hud::hud::{counts, notices, rows};

impl Client {
    /// Plays prompt-owned steps and tracks the pointer's aiming arrows.
    pub(super) fn advance_prompt(&mut self, table_now: u64) {
        let mouse = self.table_mouse();
        self.features
            .prompts
            .own(table_now, self.views, &mut self.show, &self.step_timings);
        let open = self.table_open();
        self.features.prompts.set_aims(&mut self.show, open, mouse);
    }

    /// Compiles what the frame at `now`, in nanoseconds since the client started, needs of the
    /// scripts to come (`Show::prepare`) and samples active scroll momentum. A new scroll
    /// offset may require row placement, so call this before drawing the frame. The frame
    /// also prepares the scripts for callers that omit this step.
    pub fn prepare(&mut self, now: u64) {
        self.prepare_scroll(now);
        self.prepare_story(now);
        self.layer.prepare_tip();
        self.prepare_table(self.features.bug().table_time(now));
    }

    /// From now on, compiles every script whole as it arrives (`Show::compile_whole`): for the
    /// tests that compare that with compiling over frames.
    pub fn compile_whole(&mut self) {
        self.show.compile_whole();
    }

    /// `prepare` at `table`, the table's own time (`bug`).
    pub(super) fn prepare_table(&mut self, table: u64) {
        let rows = self.show.rows_key();
        if self.show.prepare(table) {
            self.prepare_counts();
            self.prepare_notices();
        }
        if self.show.rows_key() != rows {
            self.prepare_rows();
        }
        self.prepare_steps();
    }

    /// Lays out again, for a window of a new size or a table placed again (`Show::resize`), the
    /// counts each row of cards shows, the life totals, whose type depends on the layout, and
    /// the notices of the scripts to come: a script that waited to be compiled is compiled
    /// whole as the table is placed again, and its notices are laid out only now.
    pub(super) fn prepare_resized(&mut self) {
        self.prepare_revealed_pile();
        self.prepare_rows();
        self.prepare_counts();
        self.prepare_notices();
    }

    pub(super) fn prepare_rows(&mut self) {
        self.features.update_rows(rows::Prepare {
            layer: &mut self.layer,
            rows: self.show.coming_rows(),
        });
    }

    pub(super) fn prepare_notices(&mut self) {
        self.features.notices(notices::Prepare {
            layer: &mut self.layer,
            coming: self.show.coming_notices(),
            held: self.show.notices(),
            window_width: self.window.size.width,
            size_class: self.show.table().profile().size_class(),
        });
    }

    /// Lays out every life total and pool the scripts to come will show, so that a frame lays
    /// out none of them, each total of a player the open prompt marks as a target too.
    pub(super) fn prepare_counts(&mut self) {
        self.features.totals(counts::Prepare {
            layer: &mut self.layer,
            counts: self.show.counts(),
            coming: self.show.coming_counts(),
            targets: self.features.prompts.targets(true),
            style: self.show.table().layout().play.life.text,
            passes: self.features.screen == Screen::Game,
            pools: self.features.screen != Screen::Table,
        });
    }

    /// The times at which what the prompts draw changes, or the flat layer's screen moves or
    /// fades, since `record` was called, in nanoseconds since the client started: for the
    /// interface checks' check times.
    #[must_use]
    pub fn own_times(&self) -> &[u64] {
        self.features.prompts.times()
    }

    /// Ticks the flat layer at `now` (`Layer::tick`) and returns whether a transition of its
    /// screen is still under way. While the client keeps a record, a transition that began at
    /// `now` adds its middle and end to the check times: a screen fading in can be pressed from
    /// its second frame, before the script, if any, ends. Its first frame needs no check: a
    /// node fading in is wholly transparent there, so nothing in it can be pressed, and a node
    /// moving stands where the frame before it judged it.
    pub(super) fn tick_layer(&mut self, now: u64) -> bool {
        let ticked = self.layer.tick(now);
        if ticked.began {
            self.features.prompts.record_transition(now, ticked.length);
        }
        ticked.moving
    }
}
