//! Recordings by name, played through the test driver (docs/beryl-3d.md, "The test driver"):
//! `Page::fixture` and `Page::spec`, and `check_recording`, which runs every interface check
//! on every tick of a recording. A name is a fixture's, a card spec moment's as
//! `<card>/<moment>`, a stress board's ([`crate::stress`]) or a screen's ([`crate::screens`]).

use beryl_protocol::ipc::CARD_RECORDS;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use beryl_protocol::{GameUpdate, Seat, SessionId};
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Duration, Page, Recording, TICK};
use beryl_scene::input::{Insets, Size};

use crate::faults::Listed;
use crate::runner::Test;
use crate::stress::{Board, card_records, pointer_moves, stress};
use crate::{recordings, screens};

/// The desktop windows every recording is checked in: the smallest common laptop screen, two
/// common desktop screens and an ultrawide one.
pub const DESKTOP_WINDOWS: [Size; 4] = [
    Size {
        width: 1440.0,
        height: 900.0,
    },
    Size {
        width: 1600.0,
        height: 900.0,
    },
    Size {
        width: 1920.0,
        height: 1080.0,
    },
    Size {
        width: 2560.0,
        height: 1080.0,
    },
];

/// The scale of a Retina display, physical pixels to a logical point, at which the desktop app
/// runs on the owner's Mac.
pub const RETINA_SCALE: f32 = 2.0;

/// The desktop windows of [`DESKTOP_WINDOWS`] that `just b3d test` plays at [`RETINA_SCALE`]
/// instead of 1: the client asks for its glyphs at that scale and gives its clips in physical
/// pixels, as in the desktop app on a Retina Mac, where faults appeared that no window at scale
/// 1 showed. Playing a window at scale 2 in place of 1, rather than beside it, keeps the number
/// of tests, and so the share of them a fast check has time to start, the same.
pub const RETINA_WINDOWS: [Size; 1] = [DESKTOP_WINDOWS[1]];

/// A window of the test matrix: its size in logical points and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub size: Size,
    /// Physical pixels to a logical point.
    pub scale: f32,
}

impl Window {
    /// A window of `size` at scale 1.
    #[must_use]
    pub const fn at_one(size: Size) -> Self {
        Self { size, scale: 1.0 }
    }
}

/// The phone windows every recording is checked in, in portrait: the two sizes the web
/// interface's phone shots use and a large phone's.
pub const PHONE_WINDOWS: [Size; 3] = [
    Size {
        width: 393.0,
        height: 759.0,
    },
    Size {
        width: 375.0,
        height: 647.0,
    },
    Size {
        width: 430.0,
        height: 932.0,
    },
];

/// The safe area of every phone window a recording is played in here: a status bar of 44
/// points at the top and a home indicator of 34 at the bottom, as on a phone with a notch.
/// This is test geometry, fixed so that a test gives the same faults on every machine; no
/// native host supplies it, and the phone hosts pass the device's own insets.
pub const PHONE_SAFE_AREA: Insets = Insets {
    top: 44.0,
    right: 0.0,
    bottom: 34.0,
    left: 0.0,
};

/// The safe area a page in `layout` is played with: [`PHONE_SAFE_AREA`] on the phone and
/// none on the desktop.
#[must_use]
pub fn safe_area(layout: Layout) -> Insets {
    match layout {
        Layout::Desktop => Insets::default(),
        Layout::Phone => PHONE_SAFE_AREA,
    }
}

/// Which windows of the matrix a run of the interface tests plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Windows {
    /// [`DESKTOP_WINDOWS`], each at scale 1 but [`RETINA_WINDOWS`] at [`RETINA_SCALE`], as
    /// `just b3d test` runs while the phone layout is built.
    Desktop,
    /// [`PHONE_WINDOWS`] only (`just b3d test --phone-only`).
    Phone,
    /// Both, desktop first.
    All,
}

impl Windows {
    /// The windows, in the order tests are named in.
    #[must_use]
    pub fn windows(self) -> Vec<Window> {
        let desktop = || {
            DESKTOP_WINDOWS.iter().map(|&size| Window {
                size,
                scale: if RETINA_WINDOWS.contains(&size) {
                    RETINA_SCALE
                } else {
                    1.0
                },
            })
        };
        let phone = || PHONE_WINDOWS.iter().copied().map(Window::at_one);
        match self {
            Self::Desktop => desktop().collect(),
            Self::Phone => phone().collect(),
            Self::All => desktop().chain(phone()).collect(),
        }
    }
}

/// A page for a recording found by name.
pub trait Recorded: Sized {
    /// The recording `name` (a fixture or a stress board) at its start, as `seat` sees it in
    /// the layout's usual window: 1600 by 900 on the desktop, 393 by 759 on the phone.
    ///
    /// # Panics
    ///
    /// When there is no such recording for `seat`, or it cannot be read; each says why.
    fn fixture(name: &str, seat: Side, layout: Layout) -> Self;

    /// The spec moment `moment` of the card whose spec is `card` (its slug), as
    /// [`Recorded::fixture`] plays a fixture.
    ///
    /// # Panics
    ///
    /// As [`Recorded::fixture`].
    fn spec(card: &str, moment: &str, seat: Side, layout: Layout) -> Self;
}

impl Recorded for Page {
    fn fixture(name: &str, seat: Side, layout: Layout) -> Self {
        page(name, seat, layout, usual_window(layout)).unwrap_or_else(|why| panic!("{why}"))
    }

    fn spec(card: &str, moment: &str, seat: Side, layout: Layout) -> Self {
        Self::fixture(&format!("{card}/{moment}"), seat, layout)
    }
}

/// The recording `name` at its start, as `seat` sees it in a window of `size`, with the
/// layout's [`safe_area`].
///
/// # Errors
///
/// When there is no such recording for `seat`, or it cannot be read.
pub fn page(name: &str, seat: Side, layout: Layout, size: Size) -> Result<Page, String> {
    let recording = recording(name, seat)?;
    let mut page = Page::new(recording.into(), seat, layout, size)
        .map_err(|error| format!("{name}: {error}"))?
        .named(name);
    if layout == Layout::Phone {
        // Only here, since giving a page its safe area sets its client up again.
        page = page.with_safe_area(safe_area(layout));
    }
    if let Some(board) = Board::parse(name) {
        let drive = board.drive();
        if drive.pointer_over_hand {
            let moves = pointer_moves(page.frame());
            page.set_pointer_moves(moves);
        }
        if let Some(frames) = drive.rest_before_last {
            // The reply delay applies once every view but the last has been released.
            let released = board.build().views.len() - 1;
            page.set_reply_delay(released, Duration(frames as u64 * TICK.0));
        }
    }
    Ok(page)
}

/// The messages of the recording `name` as `seat` is sent them. A stress board is seen from
/// the first seat only.
///
/// # Errors
///
/// When there is no such recording for `seat`, or it could not be recorded.
pub fn recording(name: &str, seat: Side) -> Result<Recording, String> {
    let seat = match seat {
        Side::You => Seat::P0,
        Side::Opponent => Seat::P1,
    };
    if let Some(board) = stress(name) {
        if seat != Seat::P0 {
            return Err(format!(
                "the stress board {name:?} is seen from P0 only, not {seat:?}"
            ));
        }
        let records = card_records(&board);
        let session = SessionId(1);
        let mut messages: Vec<FixtureMessage> = board
            .views
            .into_iter()
            .map(|view| FixtureMessage::game_update(&GameUpdate { session, view }))
            .collect();
        messages.insert(0, FixtureMessage::response(CARD_RECORDS, &records));
        return Ok(Recording(messages));
    }
    if let Some(messages) = screens::messages(name) {
        if seat != Seat::P0 {
            return Err(format!(
                "the screen recording {name:?} is seen from P0 only, not {seat:?}"
            ));
        }
        return Ok(Recording(messages));
    }
    recordings::find(name, seat).map(|messages| Recording(messages.to_vec()))
}

/// What [`check_recording`] played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    /// How many seat and window pairs it played.
    pub plays: usize,
    /// How many frames it checked, over all of them.
    pub frames: u64,
    /// How many pairs of consecutive frames "lines hold" compared, each sharing an arrow.
    pub line_pairs: u64,
}

/// Plays the recording `name` from each seat it is recorded for, in each of
/// [`DESKTOP_WINDOWS`], from its start to its end on its own clock, and runs every interface
/// check on the frame of every tick, and "lines hold" on each pair of consecutive frames. A
/// fault of a rule that `data/beryl3d/test-faults.json` lists for that seat and window is
/// allowed; `just b3d test` holds those to their counts.
///
/// # Panics
///
/// When a check finds a fault of a rule not listed, with the faults, the seat, the window and
/// the clock; or when the recording is recorded for neither seat, or the list cannot be read.
#[must_use]
pub fn check_recording(name: &str) -> Checked {
    let seats: Vec<Side> = [Side::You, Side::Opponent]
        .into_iter()
        .filter(|&seat| recording(name, seat).is_ok())
        .collect();
    if seats.is_empty() {
        let why = recording(name, Side::You).err().unwrap_or_default();
        panic!("{why}");
    }
    let listed = Listed::read().unwrap_or_else(|why| panic!("{why}"));
    let mut checked = Checked {
        plays: 0,
        frames: 0,
        line_pairs: 0,
    };
    for &seat in &seats {
        for size in DESKTOP_WINDOWS {
            let test = Test {
                recording: name.to_owned(),
                seat,
                size,
                scale: 1.0,
            };
            let allowed = listed.tests.get(&test.name()).cloned().unwrap_or_default();
            let mut page = page(name, seat, Layout::Desktop, size).unwrap_or_else(|why| {
                panic!("{why}");
            });
            // The arrows of the frame before, for "lines hold".
            let mut kept = Vec::new();
            loop {
                let held =
                    page.check_since(&mut kept, |fault| allowed.contains_key(fault.rule.word()));
                checked.frames += 1;
                checked.line_pairs += u64::from(held);
                if page.ended() {
                    break;
                }
                page.step(TICK);
            }
            checked.plays += 1;
        }
    }
    checked
}

/// The window a layout is usually shown in.
fn usual_window(layout: Layout) -> Size {
    match layout {
        Layout::Desktop => Size {
            width: 1600.0,
            height: 900.0,
        },
        Layout::Phone => PHONE_WINDOWS[0],
    }
}
