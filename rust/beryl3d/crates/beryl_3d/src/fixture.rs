//! `--fixture`: a recording from the recordings pack replayed in place of a game, on the same
//! clock as the test driver's (`beryl_scene::driver`). The first views are given at the first
//! frame, each later view 400 milliseconds after the table comes to rest in the view before
//! it, and a command is answered from the recording's responses. Nothing here calls the
//! session, so the frame thread can do it all: it is a lookup in memory.
//!
//! The recording's card records (its `card_records` answer) are given to the client before its
//! first views, so that the host's asset workers prepare its faces as in a live game.
//!
//! A debug build on the desktop reads the recording from the recordings pack. A phone has no
//! pack and no engine to build one, so `just b3d ios-smoke` and `android-smoke` write the
//! recording to a file on the Mac (`beryl3d --fixture <name> --write-fixture <path>`) and copy
//! it into the app's data folder, where [`Fixture::read`] finds it.

use std::path::Path;

use beryl_scene::Client;
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::driver::{REPLY_DELAY, Recording, Responses, plays_on};
use beryl_scene::frame::Wake;
use beryl_session::GameUpdate;
use beryl_session::card_records::CardRecords;
use beryl_session::ipc::FixtureMessage;
#[cfg(feature = "tools")]
use {
    beryl_inspect::pages, beryl_scene::agent::Layout, beryl_scene::driver::Moment,
    beryl_scene::input::Size, beryl_session::protocol::Side,
};

/// A moment of a replayed recording, which a bug report saved at it records in its `ui.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replayed {
    pub recording: String,
    /// `you` or `opponent`, as `just b3d`'s `--seat` takes it.
    pub seat: &'static str,
    /// Milliseconds from the recording's start, on the test driver's clock.
    pub at_ms: u64,
    /// Whether the recording had played to its end by then, so that its last moment (`end`)
    /// shows the frozen table, whatever `at_ms` says.
    pub ended: bool,
}

/// A recording being replayed.
pub struct Fixture {
    views: Vec<GameUpdate>,
    /// The recording's card records, until they are given (`cards`).
    cards: Option<CardRecords>,
    responses: Responses,
    /// How many views have been given.
    released: usize,
    /// When the next view is due, once the table rests in the last one given.
    due: Option<u64>,
    /// The recording's name and the seat it is seen from (`you` or `opponent`), for a bug
    /// report saved while it plays.
    name: String,
    seat: &'static str,
    /// The host's clock when the first views were given, in nanoseconds: the recording's
    /// moment 0.
    started: Option<u64>,
    /// The host's clock when the last script ended (`ended`), in nanoseconds.
    ended_at: Option<u64>,
}

impl Fixture {
    /// The recording `name` from the recordings pack, as `seat` sees it.
    ///
    /// # Errors
    ///
    /// When there is no such recording for `seat`, or it cannot be read.
    #[cfg(feature = "tools")]
    pub fn new(name: &str, seat: Side) -> Result<Self, String> {
        let seat_word = if seat == Side::You { "you" } else { "opponent" };
        Self::replaying(&pages::recording(name, seat)?, name, seat_word)
    }

    /// The recording `name` from the file `<name>.json` in `dir`, as `write` left it.
    ///
    /// # Errors
    ///
    /// When the file is missing or is not a recording.
    pub fn read(dir: &Path, name: &str) -> Result<Self, String> {
        let path = dir.join(format!("{name}.json"));
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let messages: Vec<FixtureMessage> = serde_json::from_str(&text)
            .map_err(|error| format!("{}: not a recording: {error}", path.display()))?;
        Self::replaying(&Recording(messages), name, "you")
    }

    fn replaying(recording: &Recording, name: &str, seat: &'static str) -> Result<Self, String> {
        Ok(Self {
            views: recording.views()?,
            cards: recording.card_records()?,
            responses: Responses::new(recording),
            released: 0,
            due: None,
            name: name.to_owned(),
            seat,
            started: None,
            ended_at: None,
        })
    }

    /// Where the replay is at the host's `clock`: the recording, its seat and the moment of it,
    /// as `just b3d tree <recording> --seat <seat> --at <ms>ms` reaches it again.
    #[must_use]
    pub fn replayed_at(&self, clock: u64) -> Replayed {
        let started = self.started.unwrap_or(clock);
        let ended = self.ended_at.is_some_and(|ended| ended <= clock);
        let at = self.ended_at.filter(|_| ended).unwrap_or(clock);
        Replayed {
            recording: self.name.clone(),
            seat: self.seat,
            at_ms: at.saturating_sub(started) / 1_000_000,
            ended,
        }
    }

    /// Notes that the recording's last script ended at the host's `clock`.
    pub fn finished_at(&mut self, clock: u64) {
        self.ended_at.get_or_insert(clock);
    }

    /// The recording's card records, the first time this is asked, for the client before the
    /// first views.
    pub fn cards(&mut self) -> Option<CardRecords> {
        self.cards.take()
    }

    /// Whether every view has been given and the table rests in the last: the recording's last
    /// script has ended.
    pub fn ended(&self, client: &Client) -> bool {
        self.released == self.views.len() && client.wake() == Wake::Never
    }

    /// The views to give the client at `clock`: the first at the start, and the next once its
    /// reply is due.
    pub fn due(&mut self, clock: u64) -> Vec<GameUpdate> {
        if self.started.is_none() {
            self.started = Some(clock);
        }
        if self.released == 0 || self.due.is_some_and(|due| due <= clock) {
            self.due = None;
            self.release()
        } else {
            Vec::new()
        }
    }

    /// The recorded answer to `command`, and the views it plays the recording on to.
    ///
    /// # Errors
    ///
    /// When the recording has no answer to it.
    pub fn answer(&mut self, command: &SessionCommand) -> Result<(Reply, Vec<GameUpdate>), String> {
        let answer = self.responses.answer(command)?;
        let views = if answer.plays_on {
            self.release()
        } else {
            Vec::new()
        };
        Ok((answer.reply, views))
    }

    /// Called after each frame: once the table rests, the next view is due 400 ms after it came
    /// to rest.
    pub fn settled(&mut self, client: &Client) {
        self.due = (self.released < self.views.len() && client.wake() == Wake::Never)
            .then(|| client.rested() + REPLY_DELAY.0);
    }

    /// When the fixture next needs a frame.
    pub fn wake(&self) -> Wake {
        if self.released == 0 {
            return Wake::Now;
        }
        self.due.map_or(Wake::Never, Wake::At)
    }

    /// The next view and every view after it that follows unasked.
    fn release(&mut self) -> Vec<GameUpdate> {
        let mut released = Vec::new();
        while let Some(update) = self.views.get(self.released) {
            self.released += 1;
            released.push(update.clone());
            if !plays_on(update) {
                break;
            }
        }
        released
    }
}

/// Writes the recording `name` from the recordings pack to `path` as JSON, for
/// [`Fixture::read`] on a phone.
///
/// # Errors
///
/// When there is no such recording, or the file cannot be written.
#[cfg(feature = "tools")]
pub fn write(name: &str, path: &Path) -> Result<(), String> {
    let recording = pages::recording(name, Side::You)?;
    let text = serde_json::to_string(&recording.0).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The time of `moment` in the recording `name` (or, with none, a duration's), in nanoseconds.
///
/// # Errors
///
/// When the moment is malformed, names a moment the recording lacks, or is a named moment with
/// no recording.
#[cfg(feature = "tools")]
pub fn moment(text: &str, name: Option<&str>, size: Size) -> Result<u64, String> {
    let moment: Moment = text
        .parse()
        .map_err(|error| format!("--pause-at {error}"))?;
    if let Moment::At(duration) = moment {
        return Ok(duration.0);
    }
    let Some(name) = name else {
        return Err(format!(
            "--pause-at {text}: a named moment needs --fixture; a game takes a duration"
        ));
    };
    let mut page = pages::page(name, Side::You, Layout::Desktop, size)?;
    page.time_of(&moment)
        .map_err(|error| format!("--pause-at {text}: {error}"))
}
