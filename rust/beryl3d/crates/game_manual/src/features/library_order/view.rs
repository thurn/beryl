use super::State;
use crate::manual::order;
use beryl_protocol::{PlayerView, protocol::ObjectView};
use kit_base::data::Card;
use kit_ui::element::Element;
use kit_ui::input::Size;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub view: &'a PlayerView,
    pub window: Size,
    /// The card whose shape the tray's boxes take.
    pub card: Card,
}

pub type ViewKey = (Size, Card, Vec<ObjectView>);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (inputs.window, inputs.card, inputs.view.objects.clone())
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    order::tray(
        state.as_ref().expect("order tray is open"),
        inputs.view,
        (inputs.window, inputs.card),
    )
}
