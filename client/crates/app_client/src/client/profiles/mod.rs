//! Profile-specific projections of the resting table for screen placement.

use app_screens::screens::Screen;
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Side, ZoneName};
use game_hud::hud::phase::{middle_band, phase_box};
use game_hud::hud::placement_scene::{Group, PlacementScene};
use game_table::table::labels::ABILITY_ROLE;
use game_table::table::{Camera, Objects};
use kit_base::data::profiles::Piece;
use kit_frame::frame::{ElementId, Role, ability_element};

const SHOWN_BOARD_IDS: u32 = 1 << 31;

impl super::Client {
    /// Projects resting objects for compact control placement. Manual controls clear
    /// the cards still showing during the transition as well as the arriving view.
    pub(super) fn placement_scene(&self) -> Option<PlacementScene> {
        let profile = self.show.table().profile();
        profile.data().hud.floating_prompt?;
        let mut layout = PlacementScene::default();
        if self.probe.is_some() {
            layout.record();
        }
        let Some(update) = self.show.latest() else {
            return Some(layout);
        };
        let table = self.show.table();
        let camera = table.camera();
        project(
            &mut layout,
            &update.view,
            self.show.latest_objects(),
            camera,
            0,
        );
        if self.features.screen == Screen::Table
            && let Some(shown) = self
                .show
                .shown()
                .filter(|shown| !std::ptr::eq(*shown, update))
        {
            // Controls move when a view arrives, while the board can still be showing
            // the previous hand and battlefield. Give both sets of cards clearance.
            project(
                &mut layout,
                &shown.view,
                table.objects(),
                camera,
                SHOWN_BOARD_IDS,
            );
        }
        if let Some(band) = middle_band(profile, &update.view).filter(|_| {
            self.features.screen == Screen::Game && profile.data().hud.table.contains(&Piece::Phase)
        }) {
            layout.phase(phase_box(band, update.view.shared.is_some()));
        }
        Some(layout)
    }
}

/// Adds each board's projected resting objects. The last displayed board uses a separate id
/// range so both positions can coexist when the same card moves between zones.
fn project(
    layout: &mut PlacementScene,
    view: &PlayerView,
    objects: &Objects,
    camera: &Camera,
    id_offset: u32,
) {
    for object in &view.objects {
        let group = match object.zone {
            ZoneName::Battlefield if object.controller == Side::You => Group::YourRows,
            ZoneName::Battlefield => Group::TheirRows,
            ZoneName::Stack => Group::Stack,
            ZoneName::Hand if object.owner == Side::You => Group::Hand,
            _ => continue,
        };
        if let Some(pose) = objects.card_pose(usize::from(object.slot.0)) {
            layout.object(
                ElementId(id_offset + u32::from(object.slot.0)),
                Role::Card,
                group,
                pose,
                camera.card_box(&pose).0,
            );
        }
    }
    for (id, pose) in objects.ability_poses() {
        layout.object(
            ElementId(id_offset + ability_element(id).0),
            ABILITY_ROLE,
            Group::Stack,
            pose,
            camera.card_box(&pose).0,
        );
    }
}
