//! What the testing screen's presses do: replay a recording the host listed, from the seat the
//! screen's toggle shows.

use beryl_protocol::protocol::Side;

use super::Client;
use crate::command::Request;
use crate::screens::Screen;

impl Client {
    /// Asks the host to replay the `recording`th recording it listed (`Input::Recordings`)
    /// from the chosen seat, and goes to the game screen, where the replay plays. Does nothing
    /// for a number past the list.
    pub(super) fn replay(&mut self, recording: u16) {
        let Some(name) = self.recordings.get(usize::from(recording)) else {
            return;
        };
        self.requests.push_back(Request::Replay {
            name: name.clone(),
            seat: self.seat,
        });
        self.screen = Screen::Game;
        self.sheet = None;
        self.show_screen();
    }

    /// Sets the seat the testing screen replays recordings from.
    pub(super) fn set_seat(&mut self, seat: Side) {
        self.seat = seat;
        self.show_screen();
    }
}
