//! The gated boards played natively, frame by frame, for the frame gates (docs/beryl-3d.md,
//! "Performance gates"). `just b3d gate` holds each frame's work counters and load to the limits
//! in `data/beryl3d/gates.json`, the allocation gate counts the heap allocations of each
//! `Client::frame`, and the instruction-count gate sends the frames `rank` picks through
//! Callgrind.
//!
//! A board is played as a host would play it at 60 frames a second. Its first view is handed
//! over before the first frame. Each later view waits until `REST_FRAMES` frames have passed
//! since the one before and the client has nothing left to animate, so a stress board's second
//! view comes 30 frames after its first, and a recorded game's views each play out before the
//! next arrives. A recorded view that the session follows unasked (`plays_on`) arrives with the
//! views after it, as the test driver hands them over. Nothing reads a clock, so every playback
//! makes the same calls and gets the same frames.

use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::protocol::{SessionId, Side};
use beryl_scene::driver::plays_on;
use beryl_scene::frame::{Frame, Wake, Work};
use beryl_scene::input::{AssetReady, Input, Insets, Size};
use beryl_scene::{Client, Config};

use crate::pages::recording;
use crate::stress::stress;

/// The boards the frame gates hold, in the order they are played: the two stress boards, and
/// two recorded games from the first seat. `verbs-zones` moves cards between every zone, and
/// `pay-default` opens prompts, so that its worst frame includes one in which `handle` lays out
/// a prompt bar and its text.
pub const BOARDS: [&str; 4] = ["stress-255", "stress-1024", "verbs-zones", "pay-default"];

/// The time between two frames, in nanoseconds: 60 frames a second.
pub const FRAME_NANOS: u64 = 16_666_667;

/// How many frames after a view the next one may arrive, at the soonest.
pub const REST_FRAMES: usize = 30;

/// The most frames a board's script may take to play out. A script that takes longer has
/// stopped ending, which the playback reports instead of playing for ever.
const MOST_FRAMES: usize = 1200;

/// How many frames `rank` returns.
pub const RANKED: usize = 8;

/// How many frames the instruction-count gate counts one by one, in `stretch`, to check that
/// `rank` picks the most expensive.
pub const STRETCH_FRAMES: usize = RANKED + 8;

/// How many frames before the top-ranked one its stretch starts.
const STRETCH_BEFORE: usize = 4;

/// The window the boards are drawn in: a 1440 by 900 point desktop window on a display of two
/// pixels per point.
const WINDOW: Size = Size {
    width: 1440.0,
    height: 900.0,
};
const SCALE: f32 = 2.0;

/// The size in physical pixels of the target the boards are drawn into: the window at its scale.
pub const TARGET: [u32; 2] = [
    (WINDOW.width * SCALE) as u32,
    (WINDOW.height * SCALE) as u32,
];

/// What a frame was doing, for naming it in a gate's report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The board at rest, waiting for its next view.
    Rest,
    /// A frame that takes a view, or one drawn while a script plays.
    Flight,
}

impl Phase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Flight => "flight",
        }
    }
}

/// One frame of a playback: when it is and what it was doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    /// Its index in the playback, from 0.
    pub frame: usize,
    /// Its time, in nanoseconds since the client started.
    pub now: u64,
    pub phase: Phase,
}

/// A gated board being played: a client and the inputs it is handed before each frame.
pub struct Playback {
    client: Client,
    /// The views still to hand over, each group handed over before one frame, last group
    /// first.
    views: Vec<Vec<GameUpdate>>,
    /// The index of the next frame.
    next: usize,
    /// The frame before which the latest view was handed over.
    last_view: Option<usize>,
}

impl Playback {
    /// Starts playing the gated board called `name`, or returns `None` for a name that is not
    /// one of `BOARDS`.
    ///
    /// # Panics
    ///
    /// When a recorded board cannot be read from the recordings pack.
    #[must_use]
    pub fn new(name: &str) -> Option<Self> {
        Self::in_window(name, WINDOW, SCALE)
    }

    /// `new`, in a window of `window` points at `scale` pixels per point instead of the gates'
    /// own, for timing frames at another size.
    ///
    /// # Panics
    ///
    /// When a recorded board cannot be read from the recordings pack.
    #[must_use]
    pub fn in_window(name: &str, window: Size, scale: f32) -> Option<Self> {
        if !BOARDS.contains(&name) {
            return None;
        }
        let (mut views, records) = match stress(name) {
            Some(board) => {
                let views = board
                    .views
                    .into_iter()
                    .map(|view| {
                        vec![GameUpdate {
                            session: SessionId(1),
                            view,
                        }]
                    })
                    .collect();
                // The stress boards' permanents have names of their own, which no card in the
                // store has, so their records hold only the card back and the nameless token's
                // art.
                (views, CardRecords::none())
            }
            None => recorded(name),
        };
        // Handed over from the back, so that taking one never moves the rest.
        views.reverse();
        let mut client = Client::new(Config::LARGEST);
        client.handle(Input::Surface {
            size: window,
            scale,
            safe_area: Insets::default(),
        });
        // As a host whose asset workers had every face ready before the first frame, so that
        // the frames are drawn with their faces, as in the app.
        client.handle(Input::Cards(Box::new(records)));
        let ready: Vec<AssetReady> = client.faces().all_ready().collect();
        for ready in ready {
            client.handle(Input::Assets(ready));
        }
        Some(Self {
            client,
            views,
            next: 0,
            last_view: None,
        })
    }

    /// Whether the next group of views is handed over before the next frame: the first at
    /// once, and each later one when `REST_FRAMES` frames have passed since the one before and
    /// the client has nothing left to animate.
    fn due(&self) -> bool {
        if self.views.is_empty() {
            return false;
        }
        match self.last_view {
            None => true,
            Some(last) => self.next >= last + REST_FRAMES && self.client.wake() == Wake::Never,
        }
    }

    /// The moment of the next frame, or `None` when every view has been handed over and the
    /// client has nothing left to animate.
    ///
    /// # Panics
    ///
    /// When the playback has gone `MOST_FRAMES` frames past its latest view without its script
    /// ending.
    #[must_use]
    pub fn moment(&self) -> Option<Moment> {
        if let Some(last) = self.last_view {
            if self.views.is_empty() && self.next > last && self.client.wake() == Wake::Never {
                return None;
            }
            assert!(
                self.next - last < MOST_FRAMES,
                "the script had not played out after {MOST_FRAMES} frames"
            );
        }
        let phase = if self.due() || self.client.wake() != Wake::Never {
            Phase::Flight
        } else {
            Phase::Rest
        };
        Some(Moment {
            frame: self.next,
            now: self.next as u64 * FRAME_NANOS,
            phase,
        })
    }

    /// Hands the client the inputs due before the next frame. This and `frame` together are one
    /// frame of a host's work.
    pub fn handle(&mut self) {
        if !self.due() {
            return;
        }
        let group = self.views.pop().expect("a view is waiting");
        self.last_view = Some(self.next);
        for update in group {
            self.client.handle(Input::Update(Box::new(update)));
        }
    }

    /// Draws the next frame.
    pub fn frame(&mut self) -> &Frame {
        let now = self.next as u64 * FRAME_NANOS;
        self.next += 1;
        self.client.frame(now)
    }
}

/// The recorded game `name`'s views from the first seat, in groups of those handed over
/// together, and the card records it starts with.
///
/// # Panics
///
/// When the recording cannot be read, as `recording` says why.
fn recorded(name: &str) -> (Vec<Vec<GameUpdate>>, CardRecords) {
    let played = recording(name, Side::You).unwrap_or_else(|why| panic!("{name}: {why}"));
    let records = played
        .card_records()
        .unwrap_or_else(|why| panic!("{name}: {why}"))
        .unwrap_or_else(CardRecords::none);
    let mut groups: Vec<Vec<GameUpdate>> = Vec::new();
    let mut open = false;
    for update in played.views().unwrap_or_else(|why| panic!("{name}: {why}")) {
        let follows = plays_on(&update);
        match groups.last_mut() {
            Some(group) if open => group.push(update),
            _ => groups.push(vec![update]),
        }
        open = follows;
    }
    (groups, records)
}

/// Makes one frame of `playback`'s host work: hands over the inputs due and draws the frame.
/// Under Callgrind, the instruction-count gate collects only inside this function
/// (`--toggle-collect`), so the frames sent through it are the frames counted.
#[inline(never)]
pub fn frame_counted(playback: &mut Playback) {
    playback.handle();
    std::hint::black_box(playback.frame());
}

/// Plays `playback` to its end, making every frame's calls directly, and returns it so that the
/// caller decides when it is dropped.
#[must_use]
pub fn play_all(mut playback: Playback) -> Playback {
    while playback.moment().is_some() {
        playback.handle();
        std::hint::black_box(playback.frame());
    }
    playback
}

/// A gated board's playback with one frame to count: the instruction-count gate's workload.
pub struct Counted {
    playback: Playback,
    /// The index of the frame sent through `frame_counted`.
    frame: usize,
}

impl Counted {
    /// The board called `name`, counting the frame at index `frame`.
    ///
    /// # Panics
    ///
    /// When `name` is not one of `BOARDS`.
    #[must_use]
    pub fn frame(name: &str, frame: usize) -> Self {
        let playback = Playback::new(name).expect("a gated board");
        Self { playback, frame }
    }

    /// The board called `name`, counting the frame in place `place` (from 0) of those `rank`
    /// picks.
    ///
    /// # Panics
    ///
    /// When `name` is not one of `BOARDS` or `place` is not below `RANKED`.
    #[must_use]
    pub fn ranked(name: &str, place: usize) -> Self {
        let ranked = rank(name).expect("a gated board");
        Self::frame(name, ranked[place])
    }

    /// Plays the board up to and including the counted frame, which goes through
    /// `frame_counted` while every frame before it makes its calls directly, and returns the
    /// playback.
    ///
    /// # Panics
    ///
    /// When the board ends before the counted frame.
    #[must_use]
    pub fn play(mut self) -> Playback {
        loop {
            let moment = self.playback.moment().expect("the counted frame is played");
            if moment.frame == self.frame {
                frame_counted(&mut self.playback);
                return self.playback;
            }
            self.playback.handle();
            std::hint::black_box(self.playback.frame());
        }
    }
}

/// Plays the gated board called `name` to its end, calling `each` with every frame and its
/// moment. Returns `false` for a name that is not one of `BOARDS`.
pub fn play(name: &str, mut each: impl FnMut(Moment, &Frame)) -> bool {
    let Some(mut playback) = Playback::new(name) else {
        return false;
    };
    while let Some(moment) = playback.moment() {
        playback.handle();
        each(moment, playback.frame());
    }
    true
}

/// The `RANKED` frames of the board called `name` with the most work, most first: the frames
/// the instruction-count gate measures. A frame's work is the sum of its work counters, and of
/// two frames with the same work the earlier ranks higher. Returns `None` for a name that is
/// not one of `BOARDS`.
#[must_use]
pub fn rank(name: &str) -> Option<[usize; RANKED]> {
    let mut works = Vec::new();
    if !play(name, |moment, frame| {
        works.push((total(frame.work), moment.frame));
    }) {
        return None;
    }
    works.sort_by_key(|&(work, frame)| (std::cmp::Reverse(work), frame));
    let mut ranked = [0; RANKED];
    for (place, &(_, frame)) in ranked.iter_mut().zip(&works) {
        *place = frame;
    }
    Some(ranked)
}

/// The frames of the board called `name` whose instructions the instruction-count gate counts
/// one by one, to check that `rank` picks the most expensive (docs/beryl-3d.md, "Finding the
/// worst frame"): `STRETCH_FRAMES` frames from a few before the frame `rank` puts first. On a
/// stress board these are the last frames at rest, the first frames of the flight, which the
/// work counters rank highest, and the flight frames after them. Returns `None` for a name that
/// is not one of `BOARDS`.
#[must_use]
pub fn stretch(name: &str) -> Option<std::ops::Range<usize>> {
    let start = rank(name)?[0].saturating_sub(STRETCH_BEFORE);
    Some(start..start + STRETCH_FRAMES)
}

/// The sum of a frame's work counters.
fn total(work: Work) -> u64 {
    [
        work.elements,
        work.segments,
        work.records,
        work.layout_nodes,
        work.text_runs,
    ]
    .into_iter()
    .map(u64::from)
    .sum()
}
