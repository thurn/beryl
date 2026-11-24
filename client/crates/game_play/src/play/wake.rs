//! When the show needs its next frame (docs/beryl-3d.md, "The frame": a frame is drawn only
//! when something moves).

use super::Show;
use kit_frame::frame::Wake;
use kit_space::motion::Track;

impl Show {
    /// When the next frame is needed: now while a segment, streak, effect, notice, span's fade,
    /// rising or falling hand card or row's slide is under way or a script waits to start, at
    /// the start of the next segment, effect, notice or fade or the end of the script while all
    /// holds still, and never when nothing plays.
    ///
    /// A span's fades count because a prompt begins and ends its marks and arrows itself, in no
    /// script (`prompts::own`): the eligible marks a priority prompt opens with come up a beat
    /// after the table rests, and without them the table counted as still before they showed.
    #[must_use]
    pub fn wake(&self, now: u64) -> Wake {
        if self.pointer.moving(now, &self.table.data().pointer)
            || self.table.sliding(now)
            || self.table.tray_moving(now)
            || self.table.shown_moving(now)
        {
            return Wake::Now;
        }
        let mut next = None::<u64>;
        let effects = (self.effects.iter()).map(|run| (run.start, run.start + run.duration));
        let notices =
            (self.notices.iter()).map(|announced| (announced.over.from, announced.over.to));
        let fades = (self.lit.open().iter())
            .flat_map(|open| [Some(open.begins), open.ends])
            .flatten()
            .map(|over| (over.from, over.to));
        for (start, end) in effects.chain(notices).chain(fades) {
            if start > now {
                next = Some(next.map_or(start, |next| next.min(start)));
            } else if now < end {
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
        // A script still compiling over frames has no end yet, and `prepare` compiles more of
        // it each frame.
        if self.recompiled.is_some() || playing.part.is_some() {
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
        // A streak is drawn on every frame while it runs, though it moves no card.
        for run in &compiled.streaks {
            if run.over.from <= t && t < run.over.to {
                return Wake::Now;
            }
            if run.over.from > t {
                next = next.min(run.over.from);
            }
        }
        // A sound is listed in the first frame at its time, so a frame is due then.
        if let Some(run) = compiled.sounds.get(playing.sounds_given) {
            next = next.min(run.at);
        }
        if next <= t {
            Wake::Now
        } else {
            Wake::At(playing.real(next))
        }
    }
}
