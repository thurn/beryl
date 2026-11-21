//! Scripts compiled over several frames instead of in the one that receives them
//! (docs/beryl-3d.md, "Threads"). Compiling a long script whole cost the frame whose `handle`
//! took it more than any other frame of most gated boards, and a frame that took many updates at
//! once compiled them all. So a script is compiled a few steps at a time instead: `Show::prepare`,
//! which a host calls before each `frame`, compiles every step that begins by the frame's time
//! and every step that could change a segment under way then (`Compiling::advance`), and a few
//! more.
//!
//! - A script that arrives with nothing playing or waiting before it starts compiling as it
//!   arrives (`in_parts`).
//! - A script that arrives behind another waits uncompiled (`defer`). The table it is played from
//!   is the one the script before it ends with, which is known only once that script is compiled
//!   whole, so it begins compiling once the scripts before it are whole and it starts by the
//!   frame's time (`begin_starting`). Until then the frames read its `finals`, worked out from the
//!   table the script before it ends with as `replay` finds it, without playing a step.
//! - A script that needs every step as it starts is compiled whole when it begins: one laid out
//!   again as it starts (`relay`), one handed a card the pointer or the tray holds, one with a
//!   planted fault, and one that plays no step over the table.
//!
//! While a script is incomplete, the frames read what its steps so far made, as they read a
//! script compiled whole. Its end is not known, so it does not end, and the show needs a frame at
//! once. Anything that needs the whole script compiles the rest of it first (`Show::complete`):
//! a new window or a scrolled row.
//!
//! Every pose at every frame is the one compiling the whole script gives. Two things the frames
//! read can differ until the script is whole. Its `finals` are worked out from its view
//! (`finals_at`). And an object gets a track only once a step that moves it is compiled, where
//! compiling the whole script gives that track a still segment from the script's start: a card
//! that changes zones in a later step is drawn as it rests, in its old zone, until then, where
//! the whole script draws its new record at the same pose from the start.

use std::hash::{Hash, Hasher};
use std::rc::Rc;

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::PlayerView;
use beryl_protocol::script::{StepTiming, Table as Played};
use self_cell::self_cell;

use super::compile::{Compiled, Compiling, STEPS_PER_ADVANCE, ended, numbered};
use super::playing::{Waiting, timings_of};
use super::step::numbers_of;
use super::{Show, rate_for};
use game_table::table::{Objects, Table};

/// What a script compiled in parts is compiled from: the view it is played from, its own
/// update and timings, the table the script before it ended with, and whether the table shows
/// that view at rest (`compile`'s `shown`). `Compiling` borrows from them, so they are held
/// beside it.
pub(super) struct Inputs {
    pub(super) before: Rc<GameUpdate>,
    pub(super) after: Rc<GameUpdate>,
    pub(super) timings: Option<Rc<[StepTiming]>>,
    pub(super) from: Rc<Played>,
    pub(super) shown: bool,
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
                (records, inputs.shown),
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

/// The table `after`'s script ends with, played from `from`, the table the script before it
/// ended with, as `replay` works it out without playing a step over the table.
fn ends_with(after: &PlayerView, from: Option<&Played>) -> Played {
    match after.script.as_ref() {
        Some(script) => ended(from, after, script),
        None => numbered(Played::of(after)),
    }
}

/// The scene objects at rest in `after`, the view a script leads to, ending with `played`
/// (`ends_with`), for the frames to read until the script is compiled whole, when they read the
/// `finals` compiling gives (`Compiling::finish`). It places `after` as the table shows a view at
/// rest, with the abilities on the stack numbered as `played` has them. Compiling places again
/// only the groups the steps change, so it can differ from this in which card tops a pile whose
/// order a step set without moving a card, never in where a card rests.
fn finals_at(after: &PlayerView, played: &Played, table: &Table) -> Objects {
    let numbers = numbers_of(played);
    let placement = table.placement(after, &numbers);
    Table::objects_for(after, numbers, &placement)
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
            shown: true,
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
            pending: false,
        };
        if part.done() {
            let (rest, played) = part.finish(&self.table, &mut waiting.compiled);
            self.count_compiled(rest);
            self.played = Some(played);
            self.unplaced += waiting.compiled.unplaced;
            self.write_record(number, &waiting.compiled);
            return Some(waiting);
        }
        // The next script to arrive is played from the table this one ends with, worked out
        // until this one is whole, when `hand_on` gives the next the table compiling ends with.
        let played = ends_with(&update.view, Some(from));
        waiting.compiled.finals = finals_at(&update.view, &played, &self.table);
        self.played = Some(played);
        waiting.part = Some(part);
        Some(waiting)
    }

    /// `update`'s script, the `number`th, which arrives behind another and waits uncompiled until
    /// it starts (`begin`), played from `from`, the table the script before it ends with as far
    /// as the show knows it now.
    pub(super) fn defer(
        &mut self,
        number: usize,
        update: Rc<GameUpdate>,
        timings: Option<Rc<[StepTiming]>>,
        from: Option<Rc<Played>>,
    ) -> Waiting {
        let played = ends_with(&update.view, from.as_deref());
        let mut compiled = Compiled {
            // Its end is not known until it is compiled whole.
            end: u64::MAX,
            ..Compiled::default()
        };
        compiled.finals = finals_at(&update.view, &played, &self.table);
        self.played = Some(played);
        Waiting {
            number,
            update,
            timings,
            from,
            compiled,
            part: None,
            pending: true,
        }
    }

    /// Compiles the steps of the playing script, and of each script that starts by `now`, that
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
        // The first script waiting starts by `now` when none plays or the playing one is whole
        // and ends by then. If it began compiling in parts before (`in_parts`, or `begin` for a
        // start foreseen too early), it may need every step now. The show plays a script whose
        // steps so far move nothing as it does one that moves nothing at all (`show_playing`),
        // which needs it whole too.
        let starts = self.playing.as_ref().is_none_or(|playing| {
            playing.part.is_none() && playing.clock(now) >= playing.compiled.end
        });
        if starts
            && let Some(waiting) = self.waiting.front()
            && waiting.part.is_some()
            && (self.needs_whole(now)
                || waiting.compiled.tracks.is_empty() && waiting.compiled.ability_tracks.is_empty())
        {
            self.complete();
        }
        let mut compiled = None;
        if let Some(playing) = &mut self.playing {
            let t = playing.clock(now);
            if let Some(part) = &mut playing.part {
                let count = part.advance(&self.table, &mut playing.compiled, t);
                compiled = Some((count, part.done()));
                self.unbuilt |= playing.compiled.unbuilt;
                playing.affect(&mut self.effects);
                playing.announce(&mut self.notices);
            }
        }
        if let Some((count, done)) = compiled {
            self.count_compiled(count);
            if done {
                self.complete();
            }
        }
        self.begin_starting(now);
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

    /// A hash of every row window the table shows and the scripts to come lead to
    /// (`coming_rows`), which changes when any of them does. It allocates nothing. A script
    /// compiled in parts reads `finals_at` until it is whole, and the finals that compiling
    /// gives can group a row's cards differently, so the client compares this before and after
    /// `prepare` to lay out the counts of cards off a row's ends again (`Client::prepare`).
    #[must_use]
    pub fn rows_key(&self) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        for rows in self.coming_rows() {
            rows.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Begins compiling each waiting script that starts by `now`, as `frame` will start them:
    /// the first as the playing script ends, or at `now` with none playing, and each after it as
    /// the one before ends, its clock running as fast as `rate_for` says for the scripts waiting
    /// then. A script still compiling has no end yet, so none after it starts. A script laid out
    /// again as it starts (`relay`) ends no earlier than its end before that, so a script after
    /// it may begin a little early, and compiles more of its steps than the frame needs.
    fn begin_starting(&mut self, now: u64) {
        let mut start = match &self.playing {
            None => now,
            Some(playing) if playing.part.is_some() => return,
            Some(playing) => {
                if playing.clock(now) < playing.compiled.end {
                    return;
                }
                playing.real(playing.compiled.end)
            }
        };
        let count = self.waiting.len();
        for index in 0..count {
            let rate = rate_for(count - index);
            self.begin(index, now, (start, rate));
            let waiting = &self.waiting[index];
            let end = waiting.compiled.end;
            if waiting.part.is_some() || now.saturating_sub(start).saturating_mul(rate) < end {
                return;
            }
            start += end.div_ceil(rate);
        }
    }

    /// Begins compiling the waiting script at `index`, if it has not begun, as it starts at
    /// `start`, in nanoseconds since the client started, its clock running `rate` times faster
    /// than real time: over frames (`part`), with the steps the frame at `now` needs, or whole
    /// where it needs every step as it starts (`needs_whole`). The scripts before it are whole,
    /// so its `from` is the table compiling the one before ended with (`hand_on`).
    pub(super) fn begin(&mut self, index: usize, now: u64, (start, rate): (u64, u64)) {
        let Some(waiting) = self.waiting.get(index).filter(|waiting| waiting.pending) else {
            return;
        };
        let number = waiting.number;
        let update = Rc::clone(&waiting.update);
        let timings = waiting.timings.clone();
        let from = waiting.from.clone();
        // It is played from the view of the script before it.
        let before = match index.checked_sub(1) {
            Some(previous) => Some(Rc::clone(&self.waiting[previous].update)),
            None => (self.playing.as_ref())
                .and_then(|playing| playing.update.clone())
                .or_else(|| self.shown.clone()),
        };
        let shown = (before.as_ref())
            .zip(self.shown.as_ref())
            .is_some_and(|(before, shown)| Rc::ptr_eq(before, shown));
        let whole = self.plants.iter().any(|plant| plant.script == number)
            || self.lays_out_again(timings_of(&update, timings.as_deref()))
            || self.pointer.carries(now);
        if let (Some(before), Some(from), false) = (&before, &from, whole) {
            let inputs = Inputs {
                before: Rc::clone(before),
                after: Rc::clone(&update),
                timings: timings.clone(),
                from: Rc::clone(from),
                shown,
            };
            let mut compiled = Compiled::default();
            let mut part = Part::new(inputs, &self.table, self.history.is_some(), &mut compiled);
            if part.moves() {
                compiled.end = u64::MAX;
                let clock = now.saturating_sub(start).saturating_mul(rate);
                let count = part.advance(&self.table, &mut compiled, clock);
                self.count_compiled(count);
                let waiting = &mut self.waiting[index];
                compiled.finals = std::mem::take(&mut waiting.compiled.finals);
                // The show plays a script whose steps so far move nothing as it does one that
                // moves nothing at all (`show_playing`), which needs it whole.
                let whole =
                    part.done() || compiled.tracks.is_empty() && compiled.ability_tracks.is_empty();
                waiting.compiled = compiled;
                waiting.part = Some(part);
                waiting.pending = false;
                if whole {
                    self.complete_waiting(index);
                }
                return;
            }
        }
        let (compiled, played) = self.compile(
            number,
            before.as_ref().map(|before| &before.view),
            from.as_deref(),
            &update,
            timings_of(&update, timings.as_deref()),
        );
        self.unplaced += compiled.unplaced;
        self.write_record(number, &compiled);
        let waiting = &mut self.waiting[index];
        waiting.compiled = compiled;
        waiting.pending = false;
        self.hand_on(number, played);
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

    /// Compiles every step left of each script compiled in parts, playing or waiting, in order,
    /// and gives the table each ends with to the script after it (`hand_on`). A script whose
    /// compiling has not begun stays so.
    pub(super) fn complete(&mut self) {
        if let Some(playing) = &mut self.playing
            && let Some(mut part) = playing.part.take()
        {
            // The effects and notices of the steps compiled now are given before `finish`
            // sorts them in with those given already.
            let count = part.advance(&self.table, &mut playing.compiled, u64::MAX);
            playing.affect(&mut self.effects);
            playing.announce(&mut self.notices);
            let (rest, played) = part.finish(&self.table, &mut playing.compiled);
            playing.effects_given = playing.compiled.effects.len();
            self.unbuilt |= playing.compiled.unbuilt;
            let number = playing.number;
            self.unplaced += playing.compiled.unplaced;
            self.count_compiled(count + rest);
            if self.history.is_some()
                && let Some(playing) = self.playing.take()
            {
                self.write_record(number, &playing.compiled);
                self.playing = Some(playing);
            }
            self.hand_on(number, played);
        }
        for index in 0..self.waiting.len() {
            self.complete_waiting(index);
        }
    }

    /// Compiles every step left of the waiting script at `index`, if it is compiled in parts.
    fn complete_waiting(&mut self, index: usize) {
        let Some(waiting) = self.waiting.get_mut(index) else {
            return;
        };
        let Some(part) = waiting.part.take() else {
            return;
        };
        let (count, played) = part.finish(&self.table, &mut waiting.compiled);
        let number = waiting.number;
        self.unplaced += waiting.compiled.unplaced;
        self.count_compiled(count);
        if self.history.is_some() {
            let compiled = std::mem::take(&mut self.waiting[index].compiled);
            self.write_record(number, &compiled);
            self.waiting[index].compiled = compiled;
        }
        self.hand_on(number, played);
    }

    /// Gives `played`, the table the `number`th script ends with once compiled whole, to the
    /// script after it, which is played from it: a waiting script whose compiling has not begun,
    /// or the next script to arrive.
    fn hand_on(&mut self, number: usize, played: Played) {
        if let Some(next) = self
            .waiting
            .iter_mut()
            .find(|next| next.number == number + 1)
        {
            if next.pending {
                next.from = Some(Rc::new(played));
            }
        } else if number + 1 == self.given {
            self.played = Some(played);
        }
    }

    /// Adds `count` to the steps compiled since `take_compiled` was last called.
    pub(super) fn count_compiled(&self, count: usize) {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        self.compiled.set(self.compiled.get().saturating_add(count));
    }
}
