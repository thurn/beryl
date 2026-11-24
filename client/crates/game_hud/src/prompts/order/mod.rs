//! The order prompt and the steps it makes on the table.

mod view;

use super::{Shown, Steps};
pub use view::{carried_item, cell_of, slot_at};

/// The marks and arrows the client draws itself for an order: none, since a waiting trigger is
/// not on the table and the session marks nothing.
pub(super) fn own(_shown: Option<Shown>, _was: Option<Shown>, _steps: &mut Steps) {}

prompt_feature!(Order, view::view, own, view::details);

/// The drop a press on an order row sends, also used to start a drag.
#[must_use]
pub fn click(action: &kit_ui::element::Action) -> Option<&beryl_protocol::prompt::Click> {
    action
        .downcast_ref::<super::feature::Message<Order>>()
        .map(|message| &message.0)
}
