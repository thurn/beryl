//! Settings and stops: what a change of either asks of the host and the session.

use beryl_protocol::protocol::{Step, StepSet};

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
        }
        self.show_screen();
    }

    /// Asks the host to save the settings after a stop changed, and sends the stops to the
    /// session.
    pub(super) fn save_stops(&mut self) {
        self.requests
            .push_back(Request::SaveSettings(self.settings.clone()));
        self.send_stops();
        self.show_screen();
    }

    /// Sends the player's stops to the session when the game holds others (`stops_held`) and
    /// the player has a decision open, as the web's `Game` does. The session takes stops only
    /// at the player's decision (`GameSession::set_stops` refuses them otherwise), so a change
    /// made while the opponent acts waits for the next one, and a game resumed or started
    /// takes up the saved stops at its first decision.
    pub(super) fn send_stops(&mut self) {
        let (Some(session), Some(held), Some(_)) = (self.session, self.stops_held, &self.prompt)
        else {
            return;
        };
        let stops = self.settings.stops;
        if held != stops {
            self.stops_held = Some(stops);
            self.commands.push_back(SessionCommand::SetStops(session, stops));
        }
    }
}

/// `set` with `step` added if it was not in it, or taken out if it was, together with the other
/// combat damage step when it is one of the two.
pub(super) fn toggled(set: StepSet, step: Step) -> StepSet {
    // Combat's two damage steps share one stop, as the web's `STEP_STOPS` has it.
    let group: &[Step] = match step {
        Step::FirstStrikeDamage | Step::CombatDamage => {
            &[Step::FirstStrikeDamage, Step::CombatDamage]
        }
        _ => &[step],
    };
    let mut steps: Vec<Step> = set.steps().map(Step::from).collect();
    if group.iter().any(|step| steps.contains(step)) {
        steps.retain(|held| !group.contains(held));
    } else {
        steps.extend_from_slice(group);
    }
    let steps: Vec<_> = steps.into_iter().map(Into::into).collect();
    StepSet::of(&steps)
}
