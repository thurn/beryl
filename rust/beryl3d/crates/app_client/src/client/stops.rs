//! Settings and stops: what a change of either asks of the host and the session.

use beryl_protocol::{GameUpdate, StopsSetting};
use game_hud::hud::steps;

use super::Client;
use crate::command::{Request, SessionCommand};
use crate::settings::Setting;

impl Client {
    /// Changes a setting, asks the host to save the settings, and tells it the sound levels
    /// or the session the stops when those changed.
    pub(super) fn set(&mut self, setting: Setting) {
        self.settings.apply(setting);
        self.requests
            .push_back(Request::SaveSettings(self.settings.clone()));
        match setting {
            Setting::Music(_) | Setting::Effects(_) | Setting::Muted(_) => {
                self.requests.push_back(Request::Audio {
                    music: self.settings.music,
                    effects: self.settings.effects,
                    muted: self.settings.muted,
                });
            }
            Setting::FullControl(_) => self.send_stops(),
            // The host sees the saved settings and changes the tier (`beryl_host`'s frame loop).
            Setting::Graphics(_) => {}
            Setting::FpsMonitor(_) => self.apply_frame_monitor_setting(),
        }
    }

    /// Applies a stop toggle to the saved settings and forwards it when the
    /// session accepts settings changes.
    pub(super) fn toggle_stop(&mut self, message: steps::Message) {
        let steps::Message::Toggle { step, theirs } = message;
        let set = if theirs {
            &mut self.settings.stops.theirs
        } else {
            &mut self.settings.stops.yours
        };
        *set = steps::toggled(*set, step);
        self.requests
            .push_back(Request::SaveSettings(self.settings.clone()));
        self.send_stops();
    }

    pub(super) fn receive_stops(&mut self, update: &GameUpdate) {
        let at = self
            .features
            .prompts
            .decision()
            .map(|decision| (update.session, decision));
        let held = update.view.stops;
        self.features
            .update_steps(steps::SyncStops::Receive { at, held });
        self.send_stops_from(held);
    }

    pub(super) fn send_stops(&mut self) {
        if let Some(update) = self.show.latest() {
            self.send_stops_from(update.view.stops);
        }
    }

    /// The session takes stops only at the player's decision. A setting made
    /// while the opponent acts is sent at the next decision.
    fn send_stops_from(&mut self, held: StopsSetting) {
        let (Some(session), Some(_)) = (self.session, self.features.prompts.decision()) else {
            return;
        };
        let want = self.settings.stops;
        if self
            .features
            .update_steps(steps::SyncStops::Send { held, want })
        {
            self.commands
                .push_back(SessionCommand::SetStops(session, want));
        }
    }
}
