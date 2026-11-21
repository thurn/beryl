use super::State;
use crate::manual::order;
use beryl_protocol::{PlayerView, protocol::ObjectView};
use kit_ui::element::Element;
use kit_ui::input::Size;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub view: &'a PlayerView,
    pub window: Size,
}

pub type ViewKey = (Size, Vec<ObjectView>);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (inputs.window, inputs.view.objects.clone())
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    order::tray(
        state.as_ref().expect("order tray is open"),
        inputs.view,
        inputs.window,
    )
}
