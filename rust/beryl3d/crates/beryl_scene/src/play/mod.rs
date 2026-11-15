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
//! - Each script's spans and streaks reach the table's `Lit` once the script's clock reaches
//!   them, and a script with a reset drops every span first. Effects, marks, the pointer and
//!   the shake then change what the table draws.
//! - When asked to (`record`), the show keeps a record of every script it is given, for the
//!   interface checks, and a test can plant a fault in a script before it plays (`plant`).

mod affect;
mod compile;
mod counts;
pub(crate) use compile::nanoseconds;
pub mod record;
pub(crate) mod step;

use std::collections::VecDeque;

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::PlayerView;
use beryl_protocol::script::{self, Act, StepTiming, Subject, Table as Played, Verb};
use glam::Vec2;

use self::compile::{Compiled, bit, compile};
use self::record::{Plant, ScriptRecord, StepRecord};
use crate::data::space::catch_up;
use crate::effects::{self, EffectRun};
use crate::faces::Faces;
use crate::frame::{FrameBuilder, Wake};
use crate::hud::counts::Counts;
use crate::light::{self, Lit, StreakRun};
use crate::motion::Track;
use crate::pointer::Pointer;
use crate::pose::Pose;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Dress, Motion, Table, Window, ability_track};
use crate::verbs::Interval;

/// How many spans, streaks and effects the show holds before holding more allocates.
const LIGHTS: usize = 256;

/// Where something is and how large, for the light drawn to or on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub pose: Pose,
    /// Half its width and height, in millimetres. A player's anchor is a point.
    pub half: Vec2,
}

/// The show as a frame draws it, for the writers of light and effects to ask where things are.
#[derive(Clone, Copy)]
pub struct Scene<'a> {
    show: &'a Show,
    now: u64,
}

impl Scene<'_> {
    /// Where `subject` is at the frame's time (`Show::reach`).
    #[must_use]
    pub fn reach(&self, subject: Subject) -> Option<Reach> {
        self.show.reach(subject, self.now)
    }

    /// The table, for its camera and layout.
    #[must_use]
    pub fn table(&self) -> &Table {
        &self.show.table
    }
}

/// An update and the timings of its script's steps, compiled to play after the scripts before
/// it.
struct Waiting {
    /// How many scripts the show was given before this one.
    number: usize,
    update: Box<GameUpdate>,
    timings: Vec<StepTiming>,
    /// The table the script before it ended with, which it is played from.
    from: Option<Played>,
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
    /// The table the script before it ended with, which it is played from.
    from: Option<Played>,
    compiled: Compiled,
    /// When it started, in nanoseconds since the client started.
    start: u64,
    /// How many times faster than real time its clock runs.
    rate: u64,
    /// How many of its `compiled.spans` and `compiled.streaks` have reached the table's `Lit`.
    spans_given: usize,
    streaks_given: usize,
    counts_given: usize,
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

    /// `over`, on its clock, in real time.
    fn real_interval(&self, over: Interval) -> Interval {
        Interval {
            from: self.real(over.from),
            to: self.real(over.to),
        }
    }

    /// Gives `lit` each span change and streak, and `counts` each count change, of the script
    /// whose real time is at or before `now`, in order, that it has not given yet.
    fn light(&mut self, now: u64, lit: &mut Lit, counts: &mut Counts) {
        self.give_counts(now, counts);
        while let Some(change) = self.compiled.spans.get(self.spans_given) {
            let over = self.real_interval(change.over);
            if over.from > now {
                break;
            }
            match change.end {
                script::Span::Begin => lit.begin(change.span, over),
                script::Span::End => lit.end(&change.span, over),
            }
            self.spans_given += 1;
        }
        while let Some(run) = self.compiled.streaks.get(self.streaks_given) {
            let over = self.real_interval(run.over);
            if over.from > now {
                break;
            }
            lit.streak(StreakRun { over, ..*run });
            self.streaks_given += 1;
        }
    }
}

/// The table and the scripts that lead it from view to view.
pub struct Show {
    table: Table,
    /// The match's faces and their layers, which the table draws each card from.
    faces: Faces,
    /// The view the table shows at rest.
    shown: Option<Box<GameUpdate>>,
    playing: Option<Playing>,
    waiting: VecDeque<Waiting>,
    /// The table the last script given ends with, which the next is played from. The scripts
    /// number the abilities on the stack and later steps name them by those numbers, which a
    /// table made from a view alone does not know.
    played: Option<Played>,
    /// How many steps of the scripts given so far the table could not take.
    unplaced: usize,
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
    /// The spans and streaks the table shows.
    lit: Lit,
    /// The effects playing, in nanoseconds since the client started, until they end.
    effects: Vec<EffectRun>,
    /// What the pointer is doing on the table.
    pointer: Pointer,
    /// The life totals and pools the table shows.
    counts: Counts,
}

impl Show {
    #[must_use]
    pub fn new(table: Table) -> Self {
        Self {
            table,
            faces: Faces::default(),
            shown: None,
            playing: None,
            waiting: VecDeque::new(),
            played: None,
            unplaced: 0,
            recompiled: None,
            unbuilt: 0,
            reported: 0,
            given: 0,
            rested: 0,
            history: None,
            recorded_from: 0,
            plants: Vec::new(),
            lit: Lit::with_capacity(LIGHTS),
            effects: Vec::with_capacity(LIGHTS),
            pointer: Pointer::default(),
            counts: Counts::default(),
        }
    }

    /// The effects the show holds, each in nanoseconds since the client started: those under
    /// way and those that begin later, until each ends. An effect outlives the script that
    /// began it, and a script with a reset drops them all.
    #[must_use]
    pub fn effects(&self) -> &[EffectRun] {
        &self.effects
    }

    /// The spans and streaks the table shows.
    #[must_use]
    pub fn lit(&self) -> &Lit {
        &self.lit
    }

    /// The spans and streaks the table shows, for the marks and arrows a prompt begins and ends
    /// itself.
    pub fn lit_mut(&mut self) -> &mut Lit {
        &mut self.lit
    }

    /// Where `subject` is at `now` and how large: a card where `pose` has it, an ability on
    /// the stack where `ability_pose` has it, a player at their anchor (`Table::player_pose`).
    #[must_use]
    pub fn reach(&self, subject: Subject, now: u64) -> Option<Reach> {
        let card = |pose: Pose| Reach {
            pose,
            half: Vec2::new(CARD_WIDTH, CARD_HEIGHT) * 0.5 * pose.scale,
        };
        match subject {
            Subject::Object { slot } => self.pose(usize::from(slot.0), now).map(card),
            Subject::Player { player } => Some(Reach {
                pose: self.table.player_pose(player),
                half: Vec2::ZERO,
            }),
            Subject::Ability { id } => self.ability_pose(id, now).map(card),
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

    /// Whether every script given has played out, as of the last `frame`: the table shows the
    /// latest view at rest, and that view's prompt may open.
    #[must_use]
    pub fn settled(&self) -> bool {
        self.playing.is_none() && self.waiting.is_empty()
    }

    /// Compiles `update`'s script, the `number`th the show is given, played from `before` and
    /// `from`, the table the script before it ended with, with any fault planted in it. Returns
    /// it with the table it ends with.
    fn compile(
        &self,
        number: usize,
        before: Option<&PlayerView>,
        from: Option<&Played>,
        update: &GameUpdate,
        timings: &[StepTiming],
    ) -> (Compiled, Played) {
        let plants = self.plants.iter().filter(|plant| plant.script == number);
        let records = self.history.is_some() || plants.clone().next().is_some();
        let shown = before
            .zip(self.shown.as_deref())
            .is_some_and(|(before, shown)| std::ptr::eq(before, &raw const shown.view));
        // Each script given after the first is played from the table the one before ended with;
        // the first has no view before it either.
        let before = before.zip(from);
        let (mut compiled, played) =
            compile(before, &update.view, timings, &self.table, records, shown);
        for plant in plants {
            compiled.plant(plant);
        }
        (compiled, played)
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
            // The script at `index` of the history leads to view `index + 1`.
            steps: compiled
                .steps
                .iter()
                .map(|step| StepRecord {
                    view: index + 1,
                    ..step.clone()
                })
                .collect(),
            segments: compiled.segments.clone(),
            finals: compiled.final_poses(),
            effects: compiled.effects.clone(),
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

    /// The match's faces and their layers.
    #[must_use]
    pub fn faces(&self) -> &Faces {
        &self.faces
    }

    pub fn faces_mut(&mut self) -> &mut Faces {
        &mut self.faces
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

    /// How many steps of the scripts given so far the table played over could not take. Each
    /// made the step after it place the whole table, so that its objects jumped to their places
    /// instead of moving there.
    #[must_use]
    pub fn unplaced(&self) -> usize {
        self.unplaced
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

    /// Where the ability numbered `id` is at `now`, as `pose` gives a card's: on its track while
    /// the playing script moves it, and otherwise where it rests on the stack of the view shown.
    #[must_use]
    pub fn ability_pose(&self, id: u32, now: u64) -> Option<Pose> {
        if let Some(playing) = &self.playing
            && playing.update.is_some()
            && let Some(track) = ability_track(&playing.compiled.ability_tracks, id)
            && !track.segments().is_empty()
        {
            return Some(track.pose_at(playing.clock(now)));
        }
        self.table.ability_rest(id)
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
        // The faces the script holds up to be read, for the Revealed Cards pile to draw once
        // their steps play.
        let scripted = update.view.script.iter().flat_map(|script| &script.beats);
        for step in scripted.flatten() {
            if let Act::Showcase { object } = &step.act {
                self.table.reveal(object);
            }
        }
        let from = self.played.take();
        let (compiled, played) =
            self.compile(number, self.before_next(), from.as_ref(), &update, &timings);
        self.played = Some(played);
        self.unplaced += compiled.unplaced;
        self.write_record(number, &compiled);
        self.waiting.push_back(Waiting {
            number,
            update,
            timings,
            from,
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
            let (again, _) = self.compile(
                playing.number,
                before,
                playing.from.as_ref(),
                update,
                &playing.timings,
            );
            compiled.push((playing.number, again));
            before = Some(&update.view);
        }
        for waiting in &self.waiting {
            let (again, _) = self.compile(
                waiting.number,
                before,
                waiting.from.as_ref(),
                &waiting.update,
                &waiting.timings,
            );
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
                if let Some(playing) = &mut self.playing {
                    playing.light(real, &mut self.lit, &mut self.counts);
                }
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
            let moves =
                !waiting.compiled.tracks.is_empty() || !waiting.compiled.ability_tracks.is_empty();
            if waiting.compiled.reset {
                self.lit.clear();
                self.effects.clear();
            }
            let start = ended.unwrap_or(now);
            self.record_start(waiting.number, start, rate);
            self.playing = Some(Playing {
                number: waiting.number,
                update: Some(waiting.update),
                timings: waiting.timings,
                from: waiting.from,
                compiled: waiting.compiled,
                start,
                rate,
                spans_given: 0,
                streaks_given: 0,
                counts_given: 0,
            });
            if let Some(playing) = &self.playing {
                playing.affect(&mut self.effects);
            }
            if !moves {
                self.show_playing();
            }
        }
        if let Some(playing) = &mut self.playing {
            playing.light(now, &mut self.lit, &mut self.counts);
        }
        self.lit.advance(now);
        // A run is under way from its start until, not at, its end, as on the web stage.
        self.effects.retain(|run| run.start + run.duration > now);
        let Some(shown) = self.shown.as_ref().map(|update| &update.view) else {
            return;
        };
        let motion = self.playing.as_ref().and_then(|playing| {
            let update = playing.update.as_ref()?;
            Some(Motion {
                tracks: &playing.compiled.tracks,
                ability_tracks: &playing.compiled.ability_tracks,
                t: playing.clock(now),
                finals: &playing.compiled.finals,
                view: &update.view,
            })
        });
        let dress = Dress {
            effects: &self.effects,
            lit: &self.lit,
            pointer: &self.pointer,
            now,
        };
        self.table
            .write(shown, motion, &self.faces, &dress, builder);
        let scene = Scene { show: self, now };
        light::arrows::write(&self.lit, now, &scene, builder);
        light::marks::write(&self.lit, now, &scene, builder);
        light::streaks::write(&self.lit, now, &scene, builder);
        effects::light::write(&self.effects, now, &scene, builder);
        builder.shake(effects::shake::offset(&self.effects, now));
    }

    /// Ends the playing script: the table shows its view, at rest, with its life totals and
    /// pools.
    fn finish(&mut self) {
        self.show_playing();
        self.playing = None;
        if let Some(shown) = &self.shown {
            self.counts.take_view(&shown.view);
        }
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
        for (id, track) in &mut compiled.ability_tracks {
            let Some(was) = ability_track(&playing.compiled.ability_tracks, *id)
                .filter(|old| !old.segments().is_empty())
            else {
                continue;
            };
            let pose = was.pose_at(t);
            let begun = track
                .segments()
                .partition_point(|segment| segment.start <= t);
            if let Some(latest) = begun.checked_sub(1) {
                track.segments_mut()[latest].through(t, pose);
            }
        }
        let number = playing.number;
        playing.compiled = compiled;
        if let Some(playing) = self.playing.take() {
            self.write_record(number, &playing.compiled);
            self.playing = Some(playing);
        }
    }

    /// When the next frame is needed: now while a segment or an effect is under way or a script
    /// is waiting to start, at the start of the next segment or effect or the end of the script
    /// while all holds still, and never when nothing plays.
    #[must_use]
    pub fn wake(&self, now: u64) -> Wake {
        let mut next = None::<u64>;
        for run in &self.effects {
            if run.start > now {
                next = Some(next.map_or(run.start, |next| next.min(run.start)));
            } else if now < run.start + run.duration {
                return Wake::Now;
            }
        }
        match (self.script_wake(now), next) {
            (Wake::Now, _) => Wake::Now,
            (Wake::At(at), Some(next)) => Wake::At(at.min(next)),
            (Wake::At(at), None) => Wake::At(at),
            (Wake::Never, Some(next)) => Wake::At(next),
            (Wake::Never, None) => Wake::Never,
        }
    }

    /// When the scripts need the next frame, as `wake` says, leaving the effects aside.
    fn script_wake(&self, now: u64) -> Wake {
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
        let abilities = compiled.ability_tracks.iter().map(|(_, track)| track);
        for segment in compiled
            .tracks
            .iter()
            .chain(abilities)
            .flat_map(Track::segments)
        {
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
