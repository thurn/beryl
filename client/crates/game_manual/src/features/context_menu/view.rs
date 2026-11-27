use super::State;
use game_table::{profiles::Profile, table::Window};
use kit_base::data::profiles::SlotName;
use kit_base::geometry::Rect;
use kit_frame::frame::Role;
use kit_ui::element::Element;
use kit_ui::{
    element::{Action, Align, Overflow, Shown, column},
    input::{FaceId, FaceSize},
    widgets::{context_menu, menu, sheet_menu},
};

use super::Message;
use crate::manual::Target;
use beryl_protocol::protocol::Facing;
use game_hud::hud::HudIn;
use glam::Vec2;

#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub window: Window,
    pub profile: Profile<'a>,
    pub face: Option<FaceId>,
}

pub type ViewKey = (Window, Option<Rect>, Option<FaceId>, Option<f32>);
pub(super) fn key(inputs: &Inputs<'_>) -> ViewKey {
    (
        inputs.window,
        inputs
            .profile
            .slot(SlotName::ManualMenu)
            .map(|slot| slot.rect),
        inputs.face,
        inputs
            .profile
            .data()
            .manual
            .map(|data| data.menu_preview_width),
    )
}

pub(super) fn render(state: &State, inputs: Inputs<'_>) -> Element {
    let open = state.as_ref().expect("context menu is open");
    let Some(slot) = inputs.profile.slot(SlotName::ManualMenu) else {
        return context_menu(open.at, menu(&open.title, &open.whose, &open.groups));
    };
    let rect = slot.rect;
    let mut panel = sheet_menu(&open.title, &open.whose, &open.groups, open.page, |page| {
        Action::new(Message::Page(page))
    })
    .min_width(rect.width)
    .max_width(rect.width)
    .max_height(rect.height)
    .overflow(Overflow::Scroll);
    if let Some(face) = inputs.face {
        let width = inputs
            .profile
            .data()
            .manual
            .expect("validated menu controls")
            .menu_preview_width;
        let card = Element {
            shows: Some(Shown::Face(face, FaceSize::Full)),
            ..Element::new(Role::Region)
        }
        .name("preview")
        .width(width)
        .height(width * inputs.profile.card().height / inputs.profile.card().width);
        panel
            .children
            .insert(0, column().align_items(Align::Center).child(card));
    }
    context_menu(
        Vec2::new(
            rect.x,
            inputs.window.size.height - inputs.window.safe_area.bottom,
        ),
        panel,
    )
}

/// The visible face the phone menu previews above its items.
pub fn sheet_face(state: &State, hud: &HudIn<'_>) -> Option<FaceId> {
    let open = state.as_ref()?;
    hud.profile.slot(SlotName::ManualMenu)?;
    let Target::Card(slot) = open.target else {
        return None;
    };
    let object = hud
        .view?
        .objects
        .iter()
        .find(|object| object.slot == slot && object.facing == Facing::Up)?;
    hud.faces?.find(object.name.as_deref()?)
}
