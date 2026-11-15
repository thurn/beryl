//! The frame loop every host runs, apart from any window (docs/beryl-3d.md, "Hosts" and
//! "Drawing only when something changes"). An input reaches the client at once but is drawn
//! only on the next tick of the display, and ticks are asked for only while the client wants a
//! frame. Where ticks come from is a type parameter, so a test can supply them.

use beryl_scene::Client;
use beryl_scene::frame::{Frame, Wake};
use beryl_scene::input::Input;

/// A source of display ticks: a display link on macOS, winit's redraw requests elsewhere, or a
/// test's own calls.
pub trait Ticks {
    /// Starts ticks while `running` is true and stops them otherwise. Called after every batch
    /// of events, so a source may ask again on each call while running.
    fn set_running(&mut self, running: bool);
}

/// Where a frame goes once the client has made it.
pub trait Draw {
    fn draw(&mut self, frame: &Frame);
}

/// How long the event loop may sleep before it must check the client again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sleep {
    /// Until the next event or tick.
    UntilEvent,
    /// Until this time, in nanoseconds since the client started, when the client wants a frame
    /// unless an event comes first.
    Until(u64),
}

/// The client and its source of ticks.
pub struct FrameLoop<T> {
    client: Client,
    ticks: T,
}

impl<T: Ticks> FrameLoop<T> {
    pub fn new(client: Client, ticks: T) -> Self {
        Self { client, ticks }
    }

    /// Hands `input` to the client. Nothing is drawn until the next tick.
    pub fn handle(&mut self, input: Input) {
        self.client.handle(input);
    }

    /// The display is ready for a frame at `now`, in nanoseconds since the client started.
    /// Draws one if the client wants it, and says whether it did.
    pub fn tick(&mut self, now: u64, draw: &mut impl Draw) -> bool {
        if !wanted(self.client.wake(), now) {
            return false;
        }
        draw.draw(self.client.frame(now));
        true
    }

    /// Called when the event loop has handled every waiting event and is about to sleep: runs
    /// ticks while the client wants a frame at `now`, and says how long the loop may sleep.
    pub fn schedule(&mut self, now: u64) -> Sleep {
        let wake = self.client.wake();
        self.ticks.set_running(wanted(wake, now));
        match wake {
            Wake::At(at) if at > now => Sleep::Until(at),
            _ => Sleep::UntilEvent,
        }
    }

    /// The client, for draining what it asks of the platform and the session after a frame.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    pub fn ticks(&mut self) -> &mut T {
        &mut self.ticks
    }
}

/// Whether the client wants a frame at `now`.
fn wanted(wake: Wake, now: u64) -> bool {
    match wake {
        Wake::Now => true,
        Wake::At(at) => at <= now,
        Wake::Never => false,
    }
}
