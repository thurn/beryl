//! Full control's indicator in the desktop dock or the phone's middle band.

use std::convert::Infallible;

use beryl_protocol::prompt::ButtonId;
use kit_base::data::profiles::Piece;
use kit_base::geometry::Rect;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;

use super::HudIn;

mod view;

/// The position and style of a visible indicator.
#[derive(Clone, Copy, PartialEq)]
pub enum Inputs {
    Dock { area: Rect, pass_on: bool },
    MiddleBand(Rect),
}

impl Inputs {
    #[must_use]
    pub fn dock(input: &HudIn<'_>) -> Option<Self> {
        let view = input.view?;
        let layout = &input.profile.data().hud;
        if !layout.table.contains(&Piece::FullControl) || !view.stops.full_control {
            return None;
        }
        let area = input.profile.slot("full_control")?.rect;
        let pass_on = input.prompt.is_some_and(|prompt| {
            prompt
                .buttons
                .iter()
                .any(|button| button.id == ButtonId::PassOn)
        });
        Some(Self::Dock { area, pass_on })
    }

    #[must_use]
    pub fn middle_band(input: &HudIn<'_>) -> Option<Self> {
        let layout = &input.profile.data().hud;
        if !layout.table.contains(&Piece::Phase) || !input.stops.full_control {
            return None;
        }
        Some(Self::MiddleBand(input.profile.slot("phase")?.rect))
    }
}

/// The read-only full control indicator.
pub struct FullControl;

impl Feature for FullControl {
    type State = ();
    type Message = Infallible;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        (): &mut (),
        message: &Infallible,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match *message {}
    }

    fn view_key((): &(), input: &Inputs) -> Inputs {
        *input
    }

    fn view((): &(), input: Inputs) -> Element {
        match input {
            Inputs::Dock { area, pass_on } => view::dock(area, pass_on),
            Inputs::MiddleBand(area) => view::middle_band(area),
        }
    }
}
