//! The settings sheet's messages and transient state. Saved settings are view inputs.

mod view;

pub use view::{GEAR_INSET, corner};

use super::Screen;
use crate::settings::{ScreenSettings, Setting};
use game_hud::hud::log::{GameLog, Message as LogMessage};
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_scene::{Context, Feature, SizeClass, Slot};
use kit_ui::element::{Action, Element};

pub struct SettingsSheet;

#[derive(Debug, Default)]
pub struct State {
    pub confirm_concede: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Open,
    Close,
    Change(Setting),
    Concede,
    ShowLog,
    ReportBug,
    /// Resets the confirmation when navigation replaces or closes the sheet.
    Reset,
}

pub enum Command {
    Concede,
}

pub enum Request {
    Open,
    Close,
    Change(Setting),
    ReportBug,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    pub screen: Screen,
    pub settings: ScreenSettings,
    pub game_over: bool,
    pub layout: Layout,
}

/// The settings geometry and size-dependent controls for this window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    slot: Slot,
    size_class: SizeClass,
    quick_action_min_width: f32,
}

impl Layout {
    #[must_use]
    pub fn new(profile: Profile<'_>) -> Self {
        Self {
            slot: profile
                .slot("settings.sheet")
                .expect("validated settings slot"),
            size_class: profile.size_class(),
            quick_action_min_width: profile
                .data()
                .settings_quick_action_min_width
                .unwrap_or_default(),
        }
    }
}

impl Feature for SettingsSheet {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Command;
    type Request = Request;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Command, Request>,
    ) -> bool {
        match *message {
            Message::Open => context.request(Request::Open),
            Message::Close => context.request(Request::Close),
            Message::Change(setting) => context.request(Request::Change(setting)),
            Message::Concede if state.confirm_concede => {
                context.request(Request::Close);
                context.command(Command::Concede);
            }
            Message::Concede => {
                state.confirm_concede = true;
                return true;
            }
            Message::ShowLog => context.send(GameLog::action(LogMessage::Toggle)),
            Message::ReportBug => context.request(Request::ReportBug),
            Message::Reset => return std::mem::take(&mut state.confirm_concede),
        }
        false
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        *inputs
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        view::settings(state, inputs)
    }
}

/// The level carried by a slider action, and the setting its next value changes.
#[must_use]
pub fn slider_setting(action: &Action) -> Option<Setting> {
    let Message::Change(setting) = *action.downcast_ref::<Message>()? else {
        return None;
    };
    Some(setting)
}
