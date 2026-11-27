//! Full control's indicator in the desktop dock or the phone's middle band.

use kit_base::data::profiles::SlotName;
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
    Dock {
        area: Rect,
        pass_on: bool,
    },
    /// In the phone's middle band, `area`, at the end across from the phase icon: the right
    /// end, or the left end when the icon stands at the right on a table whose players share
    /// their piles (`phase::phase_box`).
    MiddleBand {
        area: Rect,
        shared: bool,
    },
}

impl Inputs {
    #[must_use]
    pub fn dock(input: &HudIn<'_>) -> Option<Self> {
        let view = input.view?;
        let layout = &input.profile.data().hud;
        if !layout.table.contains(&Piece::FullControl) || !view.stops.full_control {
            return None;
        }
        let area = input.profile.slot(SlotName::FullControl)?.rect;
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
        let view = input.view?;
        Some(Self::MiddleBand {
            area: crate::hud::phase::middle_band(input.profile, view)?,
            shared: view.shared.is_some(),
        })
    }
}

/// The read-only full control indicator.
pub struct FullControl;

impl Feature for FullControl {
    const SOURCE: &'static str = file!();

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
            Inputs::MiddleBand { area, shared } => view::middle_band(area, shared),
        }
    }
}
