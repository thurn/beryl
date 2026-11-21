use super::State;
use crate::manual::palette;
use kit_ui::element::Element;
use kit_ui::{
    input::Size,
    widgets::{FieldState, Fields},
};

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub fields: &'a Fields,
    pub window: Size,
}

pub type ViewKey = (Size, FieldState);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (inputs.window, inputs.fields.get(palette::FIELD))
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    palette::panel(
        state.as_ref().expect("command list is open"),
        inputs.fields,
        inputs.window,
    )
}
