//! Showing the flat layer's screen or sheet, and the gear that opens the settings sheet, a float
//! at the game screen's top right.

use glam::Vec2;

use super::Client;
use beryl_protocol::protocol::Status;

use crate::command::Request;
use crate::element::floats;
use crate::layout::FloatLook;
use crate::screens::{self, Screen, ScreenIn};

impl Client {
    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    pub(super) fn show_screen(&mut self) {
        self.inspector.due = true;
        let view = self.show.latest().map(|update| &update.view);
        let input = ScreenIn {
            screen: self.screen,
            window: self.window,
            view,
            prompt: self.prompt.as_ref().map(|(_, prompt)| prompt),
            settings: &self.settings,
            hovered: None,
            sheet: self.sheet,
            agents: &self.agents,
            start: &self.start,
            recordings: &self.recordings,
            seat: self.seat,
            fields: &self.fields,
            inspected: None,
            faces: Some(self.show.faces().list()),
            viewing: self.viewing.open,
            revealed: &self.viewing.revealed,
        };
        // Until the sheets are built, an open sheet takes the screen's place in the flat layer.
        // The game screen shows nothing while no prompt or viewer is open and the game goes on,
        // so that no empty panel is drawn over the table (and none is built); once the game has
        // ended it shows the result.
        let ended =
            view.is_some_and(|view| matches!(view.status, Status::Over { .. } | Status::Stopped));
        let shown = match self.sheet {
            Some(sheet) => Some(screens::sheet(sheet, &input)),
            None if self.screen == Screen::Game
                && input.prompt.is_none()
                && input.viewing.is_none()
                && !ended =>
            {
                None
            }
            None => Some(screens::screen(self.screen, &input)),
        };
        let music = screens::music(self.screen, view);
        self.layer.show(shown);
        self.place_shown();
        self.show_gear();
        if music != self.music {
            self.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// Shows the gear that opens the settings at the window's top right while the game screen
    /// is shown with no sheet over it, and takes it away otherwise.
    pub(super) fn show_gear(&mut self) {
        let gear = (self.screen == Screen::Game && self.sheet.is_none()).then(screens::gear);
        let shown = gear.is_some();
        self.layer.float(floats::GEAR, gear);
        if shown {
            self.layer.set_float_look(
                floats::GEAR,
                FloatLook {
                    point: Vec2::new(
                        self.window.size.width - screens::GEAR_INSET,
                        screens::GEAR_INSET,
                    ),
                    anchor: Vec2::new(1.0, 0.0),
                    ..FloatLook::default()
                },
            );
        }
    }
}
