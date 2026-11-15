//! `--fixture`: a recording from the recordings pack replayed in place of a game, on the same
//! clock as the test driver's (`beryl_scene::driver`). The first views are given at the first
//! frame, each later view 400 milliseconds after the table comes to rest in the view before
//! it, and a command is answered from the recording's responses. Nothing here calls the
//! session, so the frame thread can do it all: it is a lookup in memory.

use beryl_inspect::pages;
use beryl_scene::Client;
use beryl_scene::agent::Layout;
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::driver::{Moment, REPLY_DELAY, Recording, Responses, plays_on};
use beryl_scene::frame::Wake;
use beryl_scene::input::Size;
use beryl_session::GameUpdate;
use beryl_session::protocol::Side;

/// A recording being replayed.
pub struct Fixture {
    views: Vec<GameUpdate>,
    responses: Responses,
    /// How many views have been given.
    released: usize,
    /// When the next view is due, once the table rests in the last one given.
    due: Option<u64>,
}

impl Fixture {
    /// The recording `name` as the first seat sees it.
    ///
    /// # Errors
    ///
    /// When there is no such recording, or it cannot be read.
    pub fn new(name: &str) -> Result<Self, String> {
        let recording: Recording = pages::recording(name, Side::You)?;
        Ok(Self {
            views: recording.views()?,
            responses: Responses::new(&recording),
            released: 0,
            due: None,
        })
    }

    /// The views to give the client at `clock`: the first at the start, and the next once its
    /// reply is due.
    pub fn due(&mut self, clock: u64) -> Vec<GameUpdate> {
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

/// The time of `moment` in the recording `name` (or, with none, a duration's), in nanoseconds.
///
/// # Errors
///
/// When the moment is malformed, names a moment the recording lacks, or is a named moment with
/// no recording.
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
