//! The script clock: the table plays each view's script before it shows the view
//! (docs/beryl-3d.md, "Verbs", "Timing a script" and "Determinism").
//!
//! - A script is compiled whole when its update arrives (`compile`), with the timings the session
//!   sent beside it, and starts at the `now` of the next frame. Nothing here reads a duration, a
//!   delay or a wait from anywhere but those timings.
//! - Scripts never overlap. An update that arrives while a script plays waits, and its script
//!   starts as the one before ends.
//! - When more than `catch_up.double` updates are waiting as a script starts, it plays twice as
//!   fast, and past `catch_up.quadruple` four times as fast (`data/beryl3d/space.json`), so that
//!   the table catches up with a session that runs ahead of it.
//! - When the window changes shape, every resting place is worked out again and the playing
//!   script is compiled again for it. A segment under way keeps its start and duration and takes
//!   the new end pose, and its start pose is changed so that the object's pose at that instant is
//!   the same: the card does not jump, and arrives at its new place.
//! - When asked to (`record`), the show keeps a record of every script it is given, for the
//!   interface checks, and a test can plant a fault in a script before it plays (`plant`).

mod compile;
pub mod record;

use std::collections::VecDeque;

use beryl_session::GameUpdate;
use beryl_session::protocol::PlayerView;
use beryl_session::script::{StepTiming, Verb};

use self::compile::{Compiled, bit, compile};
use self::record::{Plant, ScriptRecord};
use crate::data::space::catch_up;
use crate::frame::{FrameBuilder, Wake};
use crate::motion::Track;
use crate::pose::Pose;
use crate::table::{Motion, Table, Window};

/// An update and the timings of its script's steps, compiled to play after the scripts before
/// it.
struct Waiting {
    /// How many scripts the show was given before this one.
    number: usize,
    update: Box<GameUpdate>,
    timings: Vec<StepTiming>,
    compiled: Compiled,
}

/// The script under way.
struct Playing {
    /// How many scripts the show was given before this one.
    number: usize,
    /// Its update, until the table shows it. A script that moves nothing (a reset, the first
    /// view) shows its view as it starts and then only holds the time its beats take.
    update: Option<Box<GameUpdate>>,
    timings: Vec<StepTiming>,
    compiled: Compiled,
    /// When it started, in nanoseconds since the client started.
    start: u64,
    /// How many times faster than real time its clock runs.
    rate: u64,
}

impl Playing {
    /// The time on the script's own clock at `now`.
    fn clock(&self, now: u64) -> u64 {
        now.saturating_sub(self.start).saturating_mul(self.rate)
    }

    /// The real time at which its clock reads `t`, rounded up.
    fn real(&self, t: u64) -> u64 {
        self.start + t.div_ceil(self.rate)
    }
}

/// The table and the scripts that lead it from view to view.
pub struct Show {
    table: Table,
    /// The view the table shows at rest.
    shown: Option<Box<GameUpdate>>,
    playing: Option<Playing>,
    waiting: VecDeque<Waiting>,
    /// The playing script compiled again for a window of a new shape, which the next frame
    /// takes over from the old one at its own `now`.
    recompiled: Option<Compiled>,
    /// The verbs played as instant changes so far, one bit per verb as `Verb::ALL` orders them.
    unbuilt: u32,
    /// Those that `next_unbuilt` has returned.
    reported: u32,
    /// How many scripts the show has been given.
    given: usize,
    /// When the last script to end ended, in nanoseconds since the client started.
    rested: u64,
    /// A record of every script given since `record` was called, in order.
    history: Option<Vec<ScriptRecord>>,
    /// How many scripts the show had been given when `record` was called.
    recorded_from: usize,
    /// The faults to plant in scripts not yet given.
    plants: Vec<Plant>,
}

impl Show {
    #[must_use]
    pub fn new(table: Table) -> Self {
        Self {
            table,
            shown: None,
            playing: None,
            waiting: VecDeque::new(),
            recompiled: None,
            unbuilt: 0,
            reported: 0,
            given: 0,
            rested: 0,
            history: None,
            recorded_from: 0,
            plants: Vec::new(),
        }
    }

    /// From now on, keeps a record of every script the show is given (`history`).
    pub fn record(&mut self) {
        if self.history.is_none() {
            self.history = Some(Vec::new());
            self.recorded_from = self.given;
        }
    }

    /// The record of every script given since `record` was called, in order.
    #[must_use]
    pub fn history(&self) -> &[ScriptRecord] {
        self.history.as_deref().unwrap_or_default()
    }

    /// Plants a fault in a script the show has not been given yet, to test the interface checks.
    pub fn plant(&mut self, plant: Plant) {
        self.plants.push(plant);
    }

    /// When the last script to end ended, in nanoseconds since the client started: when the
    /// table came to rest, if nothing has played since. However the frames fell, it is the same.
    #[must_use]
    pub fn rested(&self) -> u64 {
        self.rested
    }

    /// Compiles `update`'s script, the `number`th the show is given, played from `before`, with
    /// any fault planted in it.
    fn compile(
        &self,
        number: usize,
        before: Option<&PlayerView>,
        update: &GameUpdate,
        timings: &[StepTiming],
    ) -> Compiled {
        let plants = self.plants.iter().filter(|plant| plant.script == number);
        let records = self.history.is_some() || plants.clone().next().is_some();
        let mut compiled = compile(before, &update.view, timings, &self.table, records);
        for plant in plants {
            compiled.plant(plant);
        }
        compiled
    }

    /// Writes `compiled`, the `number`th script, into the history, if the show keeps one.
    fn write_record(&mut self, number: usize, compiled: &Compiled) {
        let Some(history) = &mut self.history else {
            return;
        };
        let Some(index) = number.checked_sub(self.recorded_from) else {
            return;
        };
        let mut record = ScriptRecord {
            start: None,
            rate: 1,
            end: compiled.end,
            steps: compiled.steps.clone(),
            segments: compiled.segments.clone(),
            finals: compiled.final_poses(),
        };
        match history.get_mut(index) {
            // Compiled again for a window of a new shape: it keeps when it started.
            Some(kept) => {
                record.start = kept.start;
                record.rate = kept.rate;
                *kept = record;
            }
            None => history.push(record),
        }
    }

    /// Notes in the history that the `number`th script started at `start`, its clock running
    /// `rate` times faster than real time.
    fn record_start(&mut self, number: usize, start: u64, rate: u64) {
        if let Some(history) = &mut self.history
            && let Some(record) = number
                .checked_sub(self.recorded_from)
                .and_then(|index| history.get_mut(index))
        {
            record.start = Some(start);
            record.rate = rate;
        }
    }

    #[must_use]
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The latest view: the last to arrive, which the table shows once every script before it
    /// has played.
    #[must_use]
    pub fn latest(&self) -> Option<&GameUpdate> {
        self.waiting
            .back()
            .map(|waiting| &*waiting.update)
            .or(self
                .playing
                .as_ref()
                .and_then(|playing| playing.update.as_deref()))
            .or(self.shown.as_deref())
    }

    /// The view the table shows at rest: that of the last script to end.
    #[must_use]
    pub fn shown(&self) -> Option<&GameUpdate> {
        self.shown.as_deref()
    }

    /// Where the object in `slot` is at `now`, as the last frame left the clock: on its track
    /// while the playing script moves it, and otherwise where it rests in the view shown.
    #[must_use]
    pub fn pose(&self, slot: usize, now: u64) -> Option<Pose> {
        if let Some(playing) = &self.playing
            && playing.update.is_some()
            && let Some(track) = playing.compiled.tracks.get(slot)
            && !track.segments().is_empty()
        {
            return Some(track.pose_at(playing.clock(now)));
        }
        self.table.rest(slot)
    }

    /// How many updates wait for the playing script to end.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Takes an update and the timings of its script's steps, one per step in the order they are
    /// played. It is compiled now, against the view the table will show when its turn comes, and
    /// plays after every script that arrived before it.
    pub fn push(&mut self, update: Box<GameUpdate>, timings: Vec<StepTiming>) {
        let number = self.given;
        self.given += 1;
        let compiled = self.compile(number, self.before_next(), &update, &timings);
        self.write_record(number, &compiled);
        self.waiting.push_back(Waiting {
            number,
            update,
            timings,
            compiled,
        });
    }

    /// The view the next update to arrive is played from: the last one waiting, or playing, or
    /// shown.
    fn before_next(&self) -> Option<&PlayerView> {
        self.latest().map(|update| &update.view)
    }

    /// Fits the table to a window of a new shape, and compiles every script not yet ended again
    /// for it.
    pub fn resize(&mut self, window: Window) {
        let shown = self.shown.as_ref().map(|update| &update.view);
        self.table.resize(window, shown);
        let mut before = shown;
        let mut compiled = Vec::new();
        if let Some(playing) = &self.playing
            && let Some(update) = &playing.update
        {
            let again = self.compile(playing.number, before, update, &playing.timings);
            compiled.push((playing.number, again));
            before = Some(&update.view);
        }
        for waiting in &self.waiting {
            let again = self.compile(waiting.number, before, &waiting.update, &waiting.timings);
            compiled.push((waiting.number, again));
            before = Some(&waiting.update.view);
        }
        for (number, again) in compiled {
            self.write_record(number, &again);
            if let Some(waiting) = self.waiting.iter_mut().find(|w| w.number == number) {
                waiting.compiled = again;
            } else {
                self.recompiled = Some(again);
            }
        }
    }

    /// Advances the clock to `now`, ending each script whose time is up and starting the next,
    /// and writes the table as it is at `now`. Allocates nothing.
    pub fn frame(&mut self, now: u64, builder: &mut FrameBuilder) {
        self.take_recompiled(now);
        // When the script before ended, so that the next starts then and not at a later frame.
        let mut ended = None;
        loop {
            if let Some(playing) = &self.playing {
                let end = playing.compiled.end;
                if playing.clock(now) < end {
                    break;
                }
                let real = playing.real(end);
                ended = Some(real);
                self.rested = real;
                self.finish();
            }
            let Some(waiting) = self.waiting.pop_front() else {
                break;
            };
            self.unbuilt |= waiting.compiled.unbuilt;
            let rate = match self.waiting.len() + 1 {
                n if n > catch_up::QUADRUPLE => 4,
                n if n > catch_up::DOUBLE => 2,
                _ => 1,
            };
            let moves = !waiting.compiled.tracks.is_empty();
            let start = ended.unwrap_or(now);
            self.record_start(waiting.number, start, rate);
            self.playing = Some(Playing {
                number: waiting.number,
                update: Some(waiting.update),
                timings: waiting.timings,
                compiled: waiting.compiled,
                start,
                rate,
            });
            if !moves {
                self.show_playing();
            }
        }
        let Some(shown) = self.shown.as_ref().map(|update| &update.view) else {
            return;
        };
        let motion = self.playing.as_ref().and_then(|playing| {
            let update = playing.update.as_ref()?;
            Some(Motion {
                tracks: &playing.compiled.tracks,
                t: playing.clock(now),
                finals: &playing.compiled.finals,
                view: &update.view,
            })
        });
        self.table.write(shown, motion, builder);
    }

    /// Ends the playing script: the table shows its view, at rest.
    fn finish(&mut self) {
        self.show_playing();
        self.playing = None;
    }

    /// The table shows the playing script's view at rest, if it does not already.
    fn show_playing(&mut self) {
        if let Some(playing) = &mut self.playing
            && let Some(update) = playing.update.take()
        {
            self.table.swap_objects(&mut playing.compiled.finals);
            self.shown = Some(update);
        }
    }

    /// Takes over the playing script compiled again for a new window: each segment under way at
    /// `now` keeps the object where the old one had it.
    fn take_recompiled(&mut self, now: u64) {
        let Some(mut compiled) = self.recompiled.take() else {
            return;
        };
        let Some(playing) = &mut self.playing else {
            return;
        };
        let t = playing.clock(now);
        let old = &playing.compiled.tracks;
        for (slot, track) in compiled.tracks.iter_mut().enumerate() {
            let Some(was) = old.get(slot).filter(|old| !old.segments().is_empty()) else {
                continue;
            };
            let pose = was.pose_at(t);
            let begun = track
                .segments()
                .partition_point(|segment| segment.start <= t);
            if let Some(latest) = begun.checked_sub(1) {
                track.segments_mut()[latest].through(t, pose);
            }
            // The same segment in the record of the script's segments: the last added of those
            // that begin latest by `t`.
            let latest = compiled
                .segments
                .iter_mut()
                .filter(|record| record.slot == slot && record.segment.start <= t)
                .reduce(|kept, next| {
                    if next.segment.start >= kept.segment.start {
                        next
                    } else {
                        kept
                    }
                });
            if let Some(record) = latest {
                record.segment.through(t, pose);
            }
        }
        let number = playing.number;
        playing.compiled = compiled;
        if let Some(playing) = self.playing.take() {
            self.write_record(number, &playing.compiled);
            self.playing = Some(playing);
        }
    }

    /// When the next frame is needed: now while a segment is under way or a script is waiting
    /// to start, at the start of the next segment or the end of the script while the script
    /// holds still, and never when nothing plays.
    #[must_use]
    pub fn wake(&self, now: u64) -> Wake {
        let Some(playing) = &self.playing else {
            return if self.waiting.is_empty() {
                Wake::Never
            } else {
                Wake::Now
            };
        };
        if self.recompiled.is_some() {
            return Wake::Now;
        }
        let t = playing.clock(now);
        let compiled = &playing.compiled;
        let mut next = compiled.end;
        for segment in compiled.tracks.iter().flat_map(Track::segments) {
            if segment.start <= t && t < segment.end() {
                return Wake::Now;
            }
            if segment.start > t {
                next = next.min(segment.start);
            }
        }
        if next <= t {
            Wake::Now
        } else {
            Wake::At(playing.real(next))
        }
    }

    /// The verbs of the script under way, one bit per verb in the order of `Verb::ALL`, or 0
    /// when none plays.
    #[must_use]
    pub fn playing_verbs(&self) -> u32 {
        self.playing
            .as_ref()
            .map_or(0, |playing| playing.compiled.verbs)
    }

    /// The next verb that a script played as an instant change because it is not built yet,
    /// each verb returned once, for the host to log.
    pub fn next_unbuilt(&mut self) -> Option<Verb> {
        let verb = Verb::ALL
            .iter()
            .copied()
            .find(|verb| self.unbuilt & bit(*verb) != 0 && self.reported & bit(*verb) == 0)?;
        self.reported |= bit(verb);
        Some(verb)
    }
}
