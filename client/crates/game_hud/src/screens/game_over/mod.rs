//! The result and the control that puts it aside to show the battlefield.

mod view;

use beryl_protocol::protocol::{AbilityClass, OutcomeView, PlayerView, Status};
use game_table::faces::FaceList;
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_base::data::Card;
use kit_base::data::profiles::ResultLayout;
use kit_base::data::profiles::SlotName;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::input::{FaceId, Rect};
use kit_ui::widgets::screen::ScreenColumn;

pub struct GameOver;

#[derive(Debug, Default)]
pub struct State {
    aside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Shown(bool),
    NewGame,
    Reset,
}

pub struct OpenStart;

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    window: Window,
    area: Rect,
    dock: Rect,
    column: Option<ScreenColumn>,
    layout: ResultLayout,
    card: Card,
    outcome: Option<OutcomeView>,
    culprit: Option<FaceId>,
}

impl Inputs {
    /// A stopped game has a result screen without an outcome. An ongoing game
    /// has no result screen.
    #[must_use]
    pub fn new(profile: Profile<'_>, view: Option<&PlayerView>, faces: &FaceList) -> Option<Self> {
        let outcome = match &view?.status {
            Status::Over { outcome } => Some(outcome),
            Status::Stopped => None,
            _ => return None,
        };
        let culprit = outcome.and_then(|outcome| match outcome {
            OutcomeView::Win { by: Some(by), .. } => {
                let plate = by.ability.and_then(|ability| {
                    let triggered = match ability.kind {
                        AbilityClass::Triggered => true,
                        AbilityClass::Activated => false,
                        AbilityClass::Static => return None,
                    };
                    faces.plate(&by.name, triggered, ability.nth)
                });
                plate.or_else(|| faces.find(&by.name))
            }
            _ => None,
        });
        Some(Self {
            window: profile.window(),
            area: profile.slot(SlotName::Result)?.rect,
            dock: profile.slot(SlotName::ResultDock)?.rect,
            column: profile.screen_column(),
            layout: profile.data().hud.result,
            card: profile.card(),
            outcome: outcome.cloned(),
            culprit,
        })
    }
}

impl Feature for GameOver {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = std::convert::Infallible;
    type Request = OpenStart;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, OpenStart>,
    ) -> bool {
        match message {
            Message::Shown(shown) => {
                state.aside = !shown;
                true
            }
            Message::NewGame => {
                context.request(OpenStart);
                false
            }
            Message::Reset => std::mem::take(&mut state.aside),
        }
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        inputs.clone()
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        view::game_over(state, &inputs)
    }
}
