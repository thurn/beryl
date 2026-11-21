use super::State;
use crate::manual::log;
use beryl_protocol::protocol::LogLine;
use kit_base::data::flat::ManualLog;
use kit_ui::element::Element;
pub use log::Placement;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub lines: &'a [LogLine],
    pub placement: Placement,
    pub metrics: ManualLog,
}

pub type ViewKey = (Vec<LogLine>, Placement, ManualLog);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (inputs.lines.to_vec(), inputs.placement, inputs.metrics)
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    log::log(
        inputs.lines,
        state.open,
        &state.blocked,
        inputs.placement,
        inputs.metrics,
    )
    .expect("the log has lines")
}
