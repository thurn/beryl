//! Mounts the testing screen and the playground dock, and maps their effects to the client and
//! the session: the card field's lookups and the examples' starts go to the session as
//! `SessionCommand`s, and the dock's keys (`dock::SHORTCUTS`) move between examples while one
//! is played.

use app_screens::screens::gallery;
use app_screens::screens::start::{self, StartScreen};
use app_screens::screens::testing::dock::{self, PlaygroundDock};
use app_screens::screens::testing::{self, TestingScreen};
use glam::Vec2;
use kit_base::data::profiles::SlotName;
use kit_base::tokens::size;
use kit_scene::Pressed;
use kit_scene::input::KeyResponse;
use kit_ui::element::{Action, Element};

use super::{Client, Features, Mounted, Screen, SessionCommand, ViewInputs, update};
use crate::command::Request;

impl Client {
    pub(in crate::client) fn update_dock(&mut self, message: &dock::Message) -> bool {
        update::<PlaygroundDock>(self, message)
    }
}

/// The dock's keys while it is shown over the table: next, previous and restart, each taken
/// once however long it is held.
pub(super) fn keys(client: &mut Client, pressed: Pressed) -> Option<KeyResponse<Action>> {
    let shown = client.screen_shown(Screen::Game)
        && client.features.sheet.is_none()
        && (client.features)
            .state_of::<PlaygroundDock>()
            .over(client.session);
    if !shown {
        return None;
    }
    if !pressed.first {
        return Some(KeyResponse::Handled);
    }
    let message = match pressed.index {
        0 => dock::Message::Next,
        1 => dock::Message::Previous,
        _ => dock::Message::Restart,
    };
    Some(KeyResponse::Action(Action::new(message)))
}

impl Mounted for TestingScreen {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        Some(features.slot::<Self>().view(testing::Inputs {
            window: inputs.window,
            column: inputs.profile.screen_column(),
            filter: inputs.fields.get(testing::FILTER),
            card: inputs.fields.get(testing::CARD),
        }))
    }

    fn command(_: &mut Client, command: Self::Command) {
        match command {}
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        match request {
            testing::Request::Replay { name, seat } => {
                client.requests.push_back(Request::Replay { name, seat });
                client.features.screen = Screen::Game;
                client.change_sheet(None)
            }
            testing::Request::Story(name) => {
                client.update_gallery(&gallery::Message::Open(name));
                update::<StartScreen>(client, &start::Message::Open(Screen::Gallery))
            }
            testing::Request::Find(text) => {
                (client.commands).push_back(SessionCommand::PlaygroundFind(text));
                false
            }
            testing::Request::Examples(card) => {
                (client.commands).push_back(SessionCommand::PlaygroundExamples(card));
                false
            }
            testing::Request::Play(card, index) => {
                client.update_dock(&dock::Message::Play(card, index))
            }
        }
    }
}

impl Mounted for PlaygroundDock {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        if !slot.state().over(inputs.session) {
            return None;
        }
        // Right of the game log's button where the profile has one: the button, an icon
        // button, stands in the top left corner of the log toggle's slot. Otherwise in the
        // window's top left corner, inside its safe area.
        let safe = inputs.window.safe_area;
        let at = inputs.profile.slot(SlotName::LogToggle).map_or_else(
            || {
                Vec2::new(
                    safe.left + size::START_CORNER,
                    safe.top + size::START_CORNER,
                )
            },
            |toggle| {
                let corner = toggle.rect;
                Vec2::new(corner.x + size::ICON_BUTTON + size::CORNER_GAP, corner.y)
            },
        );
        Some(slot.view(dock::Inputs {
            window: inputs.window,
            at,
        }))
    }

    fn command(client: &mut Client, command: Self::Command) {
        match command {
            dock::Command::Start { card, variant } => {
                (client.commands).push_back(SessionCommand::PlaygroundStart { card, variant });
            }
        }
    }

    fn request(client: &mut Client, request: Self::Request) -> bool {
        client.features.screen = match request {
            dock::Request::Game => Screen::Game,
            dock::Request::Testing => Screen::Testing,
        };
        client.change_sheet(None);
        true
    }
}
