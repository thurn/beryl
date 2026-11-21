use super::State;
use crate::manual::log;
use beryl_protocol::protocol::LogLine;
use kit_base::geometry::Rect;
use kit_ui::element::Element;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub lines: &'a [LogLine],
    pub folded_at: Option<Rect>,
}

pub type ViewKey = (Vec<LogLine>, Option<Rect>);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (inputs.lines.to_vec(), inputs.folded_at)
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    log::log(inputs.lines, state.open, &state.blocked, inputs.folded_at).expect("the log has lines")
}
