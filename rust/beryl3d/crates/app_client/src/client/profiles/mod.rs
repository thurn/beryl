//! Profile-specific projections of the resting table for screen placement.

use app_screens::screens::Screen;
use beryl_protocol::protocol::{Side, ZoneName};
use game_hud::hud::phase::phase_box;
use game_hud::hud::placement_scene::{Group, PlacementScene};
use kit_base::data::profiles::Piece;
use kit_frame::frame::{ABILITY_IDS, ElementId, Role};

impl super::Client {
    /// Projects the latest view's arranged objects for the phone's screen layout.
    pub(super) fn placement_scene(&self) -> Option<PlacementScene> {
        let profile = self.show.table().profile();
        profile.data().hud.floating_prompt?;
        let mut layout = PlacementScene::default();
        let Some(update) = self.show.latest() else {
            return Some(layout);
        };
        let camera = self.show.table().camera();
        let objects = self.show.latest_objects();
        for object in &update.view.objects {
            let group = match object.zone {
                ZoneName::Battlefield if object.controller == Side::You => Group::YourRows,
                ZoneName::Battlefield => Group::TheirRows,
                ZoneName::Stack => Group::Stack,
                ZoneName::Hand if object.owner == Side::You => Group::Hand,
                _ => continue,
            };
            if let Some(pose) = objects.card_pose(usize::from(object.slot.0)) {
                layout.object(
                    ElementId(u32::from(object.slot.0)),
                    Role::Card,
                    group,
                    pose,
                    camera.card_box(&pose).0,
                );
            }
        }
        for (id, pose) in objects.ability_poses() {
            layout.object(
                ElementId(ABILITY_IDS + id),
                Role::Ability,
                Group::Stack,
                pose,
                camera.card_box(&pose).0,
            );
        }
        if let Some(slot) = profile.slot("phase").filter(|_| {
            self.screen == Screen::Game && profile.data().hud.table.contains(&Piece::Phase)
        }) {
            layout.phase(phase_box(update.view.stack.is_empty(), slot.rect));
        }
        Some(layout)
    }
}
