//! Recordings by name, played through the test driver (docs/beryl-3d.md, "The test driver"):
//! `Page::fixture` and `Page::spec`, and `check_recording`, which runs every interface check
//! on every tick of a recording. A name is a fixture's, a card spec moment's as
//! `<card>/<moment>`, a stress board's ([`crate::stress`]) or a screen's ([`crate::screens`]).

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::CARD_RECORDS;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use beryl_protocol::{GameUpdate, Seat, SessionId};
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Page, Recording, TICK};
use beryl_scene::input::Size;

use crate::stress::stress;
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

/// The recording `name` at its start, as `seat` sees it in a window of `size`.
///
/// # Errors
///
/// When there is no such recording for `seat`, or it cannot be read.
pub fn page(name: &str, seat: Side, layout: Layout, size: Size) -> Result<Page, String> {
    let recording = recording(name, seat)?;
    Page::new(recording.into(), seat, layout, size)
        .map(|page| page.named(name))
        .map_err(|error| format!("{name}: {error}"))
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
        let session = SessionId(1);
        let mut messages: Vec<FixtureMessage> = board
            .views
            .into_iter()
            .map(|view| FixtureMessage::game_update(&GameUpdate { session, view }))
            .collect();
        // Like every recording in the pack, it starts with the records of the cards it shows:
        // none, since its permanents have names of their own that the card store does not.
        messages.insert(
            0,
            FixtureMessage::response(CARD_RECORDS, &CardRecords::none()),
        );
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
}

/// Plays the recording `name` from each seat it is recorded for, in each of
/// [`DESKTOP_WINDOWS`], from its start to its end on its own clock, and runs every interface
/// check on the frame of every tick.
///
/// # Panics
///
/// When a check finds a fault, with the faults, the seat, the window and the clock; or when
/// the recording is recorded for neither seat.
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
    let mut checked = Checked {
        plays: 0,
        frames: 0,
    };
    for &seat in &seats {
        for size in DESKTOP_WINDOWS {
            let mut page = page(name, seat, Layout::Desktop, size).unwrap_or_else(|why| {
                panic!("{why}");
            });
            loop {
                page.check();
                checked.frames += 1;
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
        Layout::Phone => Size {
            width: 393.0,
            height: 759.0,
        },
    }
}
