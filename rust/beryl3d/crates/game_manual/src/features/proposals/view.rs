use super::State;
use crate::manual::assists;
use beryl_protocol::protocol::ProposalView;
use game_table::{profiles::Profile, table::Window};
use kit_base::data::profiles::ProfileData;
use kit_ui::element::Anchor;
use kit_ui::element::Element;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub proposals: &'a [ProposalView],
    pub window: Window,
    pub profile: Profile<'a>,
    pub anchor: Option<Anchor>,
}

pub type ViewKey = (Vec<ProposalView>, Window, ProfileData, Option<Anchor>);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (
        inputs.proposals.to_vec(),
        inputs.window,
        inputs.profile.data().clone(),
        inputs.anchor,
    )
}

pub(super) fn render(state: State, inputs: Inputs<'_>) -> Element {
    assists::proposals(
        inputs.proposals,
        state,
        inputs.window,
        inputs.profile,
        inputs.anchor,
    )
    .expect("the panel has proposals")
}
