use super::State;
use crate::manual::ask;
use game_table::{profiles::Profile, table::Window};
use kit_base::data::profiles::ProfileData;
use kit_ui::element::Element;
use kit_ui::{
    element::Anchor,
    widgets::{FieldState, Fields},
};

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub fields: &'a Fields,
    pub window: Window,
    pub profile: Profile<'a>,
    pub anchor: Option<Anchor>,
}

pub type ViewKey = (Window, ProfileData, FieldState, Option<Anchor>);
pub(super) fn key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
    let field = &state.as_ref().expect("ask prompt is open").field;
    (
        inputs.window,
        inputs.profile.data().clone(),
        inputs.fields.get(field),
        inputs.anchor,
    )
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    ask::dock(
        state.as_ref().expect("ask prompt is open"),
        inputs.fields,
        inputs.window,
        inputs.profile,
        inputs.anchor,
    )
}
