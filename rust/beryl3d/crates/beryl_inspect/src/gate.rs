//! The gated boards played natively, frame by frame, for the frame gates (docs/beryl-3d.md,
//! "Performance gates"). `just b3d gate` holds each frame's work counters and load to the limits
//! in `data/beryl3d/gates.json`, the allocation gate counts the heap allocations of each
//! `Client::frame`, and the instruction-count gate sends the frames `rank` picks through
//! Callgrind.
//!
//! A board is played as a host would play it at 60 frames a second: the first view and
//! `REST_FRAMES` frames at rest, then the next view, with frames until its script has played
//! out. Nothing reads a clock, so every playback makes the same calls and gets the same frames.

use beryl_scene::frame::{Frame, Wake, Work};
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::{Client, Config};
use beryl_session::GameUpdate;
use beryl_session::protocol::SessionId;

use crate::stress::stress;

/// The boards the frame gates hold, in the order they are played.
pub const BOARDS: [&str; 2] = ["stress-255", "stress-1024"];

/// The time between two frames, in nanoseconds: 60 frames a second.
pub const FRAME_NANOS: u64 = 16_666_667;

/// How many frames a board is drawn at rest before its next view arrives.
pub const REST_FRAMES: usize = 30;

/// The most frames a board's script may take to play out. A script that takes longer has
/// stopped ending, which the playback reports instead of playing for ever.
const MOST_FRAMES: usize = 1200;

/// How many frames `rank` returns.
pub const RANKED: usize = 8;

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
    /// The board at rest, before its next view.
    Rest,
    /// From the frame that takes the next view until its script has played out.
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
    /// The views still to hand over, each with the frame before which it is handed.
    views: Vec<(usize, GameUpdate)>,
    /// The index of the next frame.
    next: usize,
    /// The frame before which the latest view was handed over.
    last_view: Option<usize>,
}

impl Playback {
    /// Starts playing the gated board called `name`, or returns `None` for a name that is not
    /// one of `BOARDS`.
    #[must_use]
    pub fn new(name: &str) -> Option<Self> {
        let board = stress(name)?;
        let mut views: Vec<(usize, GameUpdate)> = board
            .views
            .into_iter()
            .enumerate()
            .map(|(index, view)| {
                (
                    index * REST_FRAMES,
                    GameUpdate {
                        session: SessionId(1),
                        view,
                    },
                )
            })
            .collect();
        // Handed over from the back, so that taking one never moves the rest.
        views.reverse();
        let mut client = Client::new(Config::LARGEST);
        client.handle(Input::Surface {
            size: WINDOW,
            scale: SCALE,
            safe_area: Insets::default(),
        });
        Some(Self {
            client,
            views,
            next: 0,
            last_view: None,
        })
    }

    /// The moment of the next frame, or `None` when every view has been handed over and the
    /// client has nothing left to animate.
    ///
    /// # Panics
    ///
    /// When the playback has gone `MOST_FRAMES` frames past its last view without its script
    /// ending.
    #[must_use]
    pub fn moment(&self) -> Option<Moment> {
        if self.views.is_empty()
            && let Some(last) = self.last_view
        {
            if self.next > last && self.client.wake() == Wake::Never {
                return None;
            }
            assert!(
                self.next - last < MOST_FRAMES,
                "the script had not played out after {MOST_FRAMES} frames"
            );
        }
        let phase = if self.next < REST_FRAMES {
            Phase::Rest
        } else {
            Phase::Flight
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
        while let Some((at, _)) = self.views.last()
            && *at <= self.next
        {
            let (at, update) = self.views.pop().expect("a view is waiting");
            self.last_view = Some(at);
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
