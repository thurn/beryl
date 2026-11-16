//! A script compiled over several frames instead of in the one that receives it
//! (docs/beryl-3d.md, "Threads"). Compiling a long script whole cost the frame whose `handle`
//! took it more than any other frame of most gated boards. A script that arrives with nothing
//! playing or waiting before it, which is most of them, is compiled a few steps at a time
//! instead: `Show::prepare`, which a host calls before each `frame`, compiles every step that
//! begins by the frame's time and every step that could change a segment under way then
//! (`Compiling::advance`), and a few more.
//!
//! While the script is incomplete, the frames read what its steps so far made, as they read a
//! script compiled whole. Its `finals`, which the frames read every frame, are worked out as
//! it arrives. Its end is not known, so it does not end, and the show needs a frame at once.
//! Anything that needs the whole script compiles the rest of it first (`Show::complete`): the
//! next script to arrive, a new window or a scrolled row, and a start that lays the script out
//! again (`relay`) or hands it a card the pointer or the tray holds.
//!
//! Every pose at every frame is the one compiling the whole script gives. Two things the frames
//! read can differ until the script is whole. Its `finals` are worked out from its view
//! (`finals_of`). And an object gets a track only once a step that moves it is compiled, where
//! compiling the whole script gives that track a still segment from the script's start: a card
//! that changes zones in a later step is drawn as it rests, in its old zone, until then, where
//! the whole script draws its new record at the same pose from the start.

use std::rc::Rc;

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::PlayerView;
use beryl_protocol::script::{StepTiming, Table as Played, replay};
use self_cell::self_cell;

use super::Show;
use super::compile::{Compiled, Compiling, STEPS_PER_ADVANCE, numbered};
use super::playing::{Waiting, timings_of};
use super::step::numbers_of;
use crate::table::{Objects, Table};

/// What a script compiled in parts is compiled from: the view the table shows as it arrives,
/// its own update and timings, and the table the script before it ended with. `Compiling`
/// borrows from them, so they are held beside it.
pub(super) struct Inputs {
    pub(super) before: Rc<GameUpdate>,
    pub(super) after: Rc<GameUpdate>,
    pub(super) timings: Option<Rc<[StepTiming]>>,
    pub(super) from: Rc<Played>,
}

self_cell!(
    struct Cell {
        owner: Inputs,

        #[not_covariant]
        dependent: Compiling,
    }
);

/// A script being compiled over frames, with what it is compiled from.
pub(super) struct Part(Cell);

impl Part {
    /// Begins compiling `inputs`' script into `out`, played over `table`, from the view the
    /// table shows. With `records`, it keeps the records the interface checks read.
    pub(super) fn new(inputs: Inputs, table: &Table, records: bool, out: &mut Compiled) -> Self {
        Self(Cell::new(inputs, |inputs| {
            let timings = timings_of(&inputs.after, inputs.timings.as_deref());
            Compiling::new(
                Some((&inputs.before.view, &inputs.from)),
                (&inputs.after.view, timings),
                table,
                (records, true),
                out,
            )
        }))
    }

    /// Whether the script plays step by step (`Compiling::moves`).
    pub(super) fn moves(&self) -> bool {
        self.0.with_dependent(|_, compiling| compiling.moves())
    }

    /// Whether every step has been compiled.
    pub(super) fn done(&self) -> bool {
        self.0.with_dependent(|_, compiling| compiling.done())
    }

    /// Compiles into `out` as `Compiling::advance` does, and returns how many steps it compiled.
    pub(super) fn advance(&mut self, table: &Table, out: &mut Compiled, until: u64) -> usize {
        self.0.with_dependent_mut(|_, compiling| {
            compiling.advance(table, out, (until, STEPS_PER_ADVANCE))
        })
    }

    /// Compiles every step left into `out`, and completes it (`Compiling::finish`). Returns how
    /// many steps it compiled and the table the script ends with.
    pub(super) fn finish(mut self, table: &Table, out: &mut Compiled) -> (usize, Played) {
        self.0.with_dependent_mut(|_, compiling| {
            let count = compiling.advance(table, out, (u64::MAX, 0));
            (count, compiling.finish(table, out))
        })
    }
}

/// The scene objects at rest in `after`, the view `after`'s script leads to from `from`, the
/// table the script before it ended with, for the frames to read until the script is compiled
/// whole, when they read the `finals` compiling gives (`Compiling::finish`). It places `after`
/// as the table shows a view at rest, with the abilities on the stack numbered as the script
/// leaves them (`replay`), or from their places where the table cannot take a step. Compiling
/// places again only the groups the steps change, so it can differ from this in which card
/// tops a pile whose order a step set without moving a card, never in where a card rests.
pub(super) fn finals_of(after: &PlayerView, from: &Played, table: &Table) -> Objects {
    let whole = |played: Played| {
        let numbers = numbers_of(&played);
        let placement = table.placement(after, &numbers);
        Table::objects_for(after, numbers, &placement)
    };
    let Some(script) = after.script.as_ref() else {
        return whole(numbered(Played::of(after)));
    };
    let played = replay(from.clone(), script).unwrap_or_else(|_| numbered(Played::of(after)));
    whole(played)
}

impl Show {
    /// `update`'s script, the `number`th, ready to compile over frames (`part`), if it can be:
    /// it arrives with nothing playing or waiting, played from the view the table shows and
    /// `from`, the table the script before it ended with, it plays step by step, and no test
    /// plants a fault in it.
    pub(super) fn in_parts(
        &mut self,
        number: usize,
        update: &Rc<GameUpdate>,
        timings: Option<&Rc<[StepTiming]>>,
        from: Option<&Rc<Played>>,
    ) -> Option<Waiting> {
        let (Some(before), Some(from)) = (self.shown.clone(), from) else {
            return None;
        };
        if self.whole
            || self.playing.is_some()
            || !self.waiting.is_empty()
            || self.plants.iter().any(|plant| plant.script == number)
            || self.lays_out_again(timings_of(update, timings.map(|timings| &timings[..])))
        {
            return None;
        }
        let inputs = Inputs {
            before: Rc::clone(&before),
            after: Rc::clone(update),
            timings: timings.cloned(),
            from: Rc::clone(from),
        };
        let mut compiled = Compiled::default();
        let mut part = Part::new(inputs, &self.table, self.history.is_some(), &mut compiled);
        if !part.moves() {
            return None;
        }
        // Its end is not known until its last step is compiled, and it does not end before.
        compiled.end = u64::MAX;
        // It starts at the next frame, at its clock's 0, which needs the steps that begin then:
        // compiled now, in the same tick, they are counted with that frame. Most scripts are
        // short, and these are all their steps.
        let count = part.advance(&self.table, &mut compiled, 0);
        self.count_compiled(count);
        let mut waiting = Waiting {
            number,
            update: Rc::clone(update),
            timings: timings.cloned(),
            from: Some(Rc::clone(from)),
            compiled,
            part: None,
        };
        if part.done() {
            let (rest, played) = part.finish(&self.table, &mut waiting.compiled);
            self.count_compiled(rest);
            self.played = Some(played);
            self.unplaced += waiting.compiled.unplaced;
            self.write_record(number, &waiting.compiled);
            return Some(waiting);
        }
        waiting.compiled.finals = finals_of(&update.view, from, &self.table);
        waiting.part = Some(part);
        Some(waiting)
    }

    /// Compiles the steps of the playing script, and of the script that starts by `now`, that
    /// the frame at `now` needs, in nanoseconds since the client started, and a few more
    /// (`Compiling::advance`). A host calls it before `frame`, and it may allocate; `frame`
    /// calls it too, for the callers that do not, and does nothing more when it was called for
    /// `now` already. Returns whether the scripts to come gained a count or a notice, whose
    /// floats the client then lays out (`Client::prepare`).
    pub fn prepare(&mut self, now: u64) -> bool {
        if self.prepared == Some(now) {
            return false;
        }
        self.prepared = Some(now);
        let counts = self
            .coming_counts()
            .map(|(counts, _)| counts.len())
            .sum::<usize>();
        let notices = (self.coming_notices())
            .map(|(_, notices, _)| notices.len())
            .sum::<usize>();
        // A script compiled in parts arrived with nothing playing or waiting, so it starts at the
        // first frame after it arrived, at its clock's 0, whose steps it compiled as it arrived
        // (`in_parts`). The show plays a script whose steps so far move nothing as it does one
        // that moves nothing at all (`show_playing`), which needs it whole.
        if self.playing.is_none()
            && let Some(waiting) = self.waiting.front()
            && waiting.part.is_some()
            && (self.needs_whole(now)
                || waiting.compiled.tracks.is_empty() && waiting.compiled.ability_tracks.is_empty())
        {
            self.complete();
        }
        if let Some(playing) = &mut self.playing
            && playing.part.is_some()
        {
            let t = playing.clock(now);
            let (count, done) = playing.part.as_mut().map_or((0, false), |part| {
                (
                    part.advance(&self.table, &mut playing.compiled, t),
                    part.done(),
                )
            });
            playing.affect(&mut self.effects);
            playing.announce(&mut self.notices);
            self.count_compiled(count);
            if done {
                self.complete();
            }
        }
        let grew_counts = self
            .coming_counts()
            .map(|(counts, _)| counts.len())
            .sum::<usize>()
            != counts;
        let grew_notices = (self.coming_notices())
            .map(|(_, notices, _)| notices.len())
            .sum::<usize>()
            != notices;
        grew_counts || grew_notices
    }

    /// From now on, compiles every script whole as it arrives, never over frames: for the tests
    /// that compare the two.
    pub fn compile_whole(&mut self) {
        self.whole = true;
    }

    /// Whether the script compiled in parts that starts at `now` needs every step as it starts:
    /// it is laid out again then (`relay`), or the pointer or the tray hands it a card
    /// (`hand_over`, `hand_over_tray`, `place_let_go`).
    fn needs_whole(&self, now: u64) -> bool {
        let Some(waiting) = self.waiting.front() else {
            return false;
        };
        self.lays_out_again(timings_of(&waiting.update, waiting.timings.as_deref()))
            || self.pointer.carries(now)
    }

    /// Whether a script timed by `timings` that starts next is laid out again as it starts
    /// (`relay`) or hands a card the tray let go to a step (`hand_over_tray`, `place_let_go`):
    /// it has a step that holds a span or waits for the table to itself, or the tray is letting
    /// its cards go.
    fn lays_out_again(&self, timings: &[StepTiming]) -> bool {
        timings
            .iter()
            .any(|timing| timing.hold.is_some() || timing.alone)
            || self.table.let_go_waits_for().is_some()
            || self.table.tray_leaving()
    }

    /// Compiles every step left of the script compiled in parts, playing or waiting, if there
    /// is one, and takes the table it ends with as the one the next script plays from.
    pub(super) fn complete(&mut self) {
        let (number, played, unplaced) = if let Some(playing) = &mut self.playing
            && let Some(mut part) = playing.part.take()
        {
            // The effects and notices of the steps compiled now are given before `finish`
            // sorts them in with those given already.
            let count = part.advance(&self.table, &mut playing.compiled, u64::MAX);
            playing.affect(&mut self.effects);
            playing.announce(&mut self.notices);
            let (rest, played) = part.finish(&self.table, &mut playing.compiled);
            playing.effects_given = playing.compiled.effects.len();
            let done = (playing.number, played, playing.compiled.unplaced);
            self.count_compiled(count + rest);
            done
        } else if let Some(waiting) = self.waiting.front_mut()
            && let Some(part) = waiting.part.take()
        {
            let (count, played) = part.finish(&self.table, &mut waiting.compiled);
            let done = (waiting.number, played, waiting.compiled.unplaced);
            self.count_compiled(count);
            done
        } else {
            return;
        };
        self.played = Some(played);
        self.unplaced += unplaced;
        self.record_compiled(number);
    }

    /// Writes the `number`th script, playing or waiting, into the history, if the show keeps
    /// one.
    fn record_compiled(&mut self, number: usize) {
        if self.history.is_none() {
            return;
        }
        if let Some(playing) = self.playing.take() {
            if playing.number == number {
                self.write_record(number, &playing.compiled);
            }
            self.playing = Some(playing);
        }
        if let Some(waiting) = self.waiting.pop_front() {
            if waiting.number == number {
                self.write_record(number, &waiting.compiled);
            }
            self.waiting.push_front(waiting);
        }
    }

    /// Adds `count` to the steps compiled since `take_compiled` was last called.
    fn count_compiled(&self, count: usize) {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        self.compiled.set(self.compiled.get().saturating_add(count));
    }
}
