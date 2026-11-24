use super::State;
use crate::manual::tokens;
use beryl_protocol::{PlayerView, ipc::decks::CardTokenLink};
use kit_base::data::flat::Palette;
use kit_ui::element::Element;
use kit_ui::{
    input::Size,
    widgets::{FieldState, Fields},
};

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub view: Option<&'a PlayerView>,
    pub fields: &'a Fields,
    pub window: Size,
    pub metrics: Palette,
}

pub type ViewKey = (Size, Palette, Vec<FieldState>, Vec<CardTokenLink>);
pub(super) fn key(state: &State, inputs: &Inputs<'_>) -> ViewKey {
    let fields = if state.custom.is_some() {
        tokens::CUSTOM_FIELDS
            .iter()
            .map(|name| inputs.fields.get(name))
            .collect()
    } else {
        vec![inputs.fields.get(tokens::FIELD)]
    };
    let tokens = inputs
        .view
        .and_then(|view| view.table.as_ref())
        .map(|table| table.deck_tokens.clone())
        .unwrap_or_default();
    (inputs.window, inputs.metrics, fields, tokens)
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    if let Some(custom) = &state.custom {
        return tokens::custom_panel(custom, inputs.fields, inputs.window, inputs.metrics);
    }
    tokens::panel(
        state.search.as_ref().expect("token search is open"),
        inputs.view.expect("token search has a table view"),
        inputs.fields,
        inputs.window,
        inputs.metrics,
    )
}
