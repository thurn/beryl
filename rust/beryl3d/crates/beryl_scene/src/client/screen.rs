//! Showing the flat layer's screen or sheet, and the gear that opens the settings sheet, a float
//! at the game screen's top right.

use glam::Vec2;

use super::Client;

use crate::command::Request;
use crate::element::floats;
use crate::faces::{FaceList, Faces};
use crate::input::{AssetReady, FaceSize};
use crate::layout::FloatLook;
use crate::screens::{self, Screen, ScreenIn, Sheet};

impl Client {
    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    pub(super) fn show_screen(&mut self) {
        self.inspector.due = true;
        let view = self.show.latest().map(|update| &update.view);
        let input = ScreenIn {
            screen: self.screen,
            window: self.window,
            view,
            prompt: self.prompt.as_ref().map(|(_, prompt)| prompt),
            settings: &self.settings,
            hovered: None,
            sheet: self.sheet,
            agents: &self.agents,
            start: &self.start,
            gallery: self.gallery,
            recordings: &self.recordings,
            seat: self.seat,
            bug: &self.bug.form,
            fields: &self.fields,
            inspected: None,
            faces: Some(self.faces()),
            viewing: self.viewing.open,
            revealed: &self.viewing.revealed,
            table_menu: self.manual.menu.as_ref(),
            carried: self.carried_item.map(|carry| carry.on),
        };
        let mut full = crate::prompts::faces(&input);
        full.extend(screens::sheet_face(&input));
        // Until the sheets are built, the settings sheet takes the screen's place in the flat
        // layer; the bug report is drawn over the screen. Before a game has a view and with no
        // prompt open, the game screen shows nothing, so that no empty panel is drawn over the
        // table. Once a game has a view it shows the players' furniture, and once the game has
        // ended, the result.
        let under = match self.sheet {
            Some(sheet) if sheet != Sheet::BugReport => None,
            _ if self.bug_hides_screen() => None,
            _ if self.screen == Screen::Game && input.prompt.is_none() && input.view.is_none() => {
                None
            }
            _ => Some(screens::screen(self.screen, &input)),
        };
        let shown = match self.sheet {
            Some(sheet) => Some(screens::sheet(sheet, under, &input)),
            None => under,
        };
        let music = screens::music(self.screen, view);
        self.layer.show(shown);
        self.place_shown();
        for face in full {
            self.show.faces_mut().want_full(face);
        }
        self.show_gear();
        if music != self.music {
            self.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// The active screen's faces, which the host's asset workers prepare. A match's
    /// layers arrive as `AssetReady::Face` and the gallery's as `AssetReady::GalleryFace`.
    #[must_use]
    pub fn faces(&self) -> &FaceList {
        self.face_layers().list()
    }

    /// The layer each active face has landed in at each size.
    #[must_use]
    pub fn face_layers(&self) -> &Faces {
        if self.screen == Screen::Gallery
            && let Some(faces) = &self.gallery_faces
        {
            return faces;
        }
        self.show.faces()
    }

    /// Whether the active face list belongs to the gallery.
    #[must_use]
    pub fn showing_gallery(&self) -> bool {
        self.screen == Screen::Gallery
    }

    /// Records where the host uploaded each face, glyph or icon the client can draw.
    pub(super) fn asset_ready(&mut self, asset: AssetReady) {
        match asset {
            AssetReady::Face { face, size, layer } => {
                self.show.faces_mut().arrived(face, size, layer);
            }
            AssetReady::GalleryFace { face, layer } => {
                if let Some(faces) = &mut self.gallery_faces {
                    faces.arrived(face, FaceSize::Board, layer);
                }
            }
            AssetReady::Glyphs { page, glyphs } => {
                self.layer.glyphs_mut().add(page, &glyphs);
                self.show.badges_mut().add(page, &glyphs);
            }
            AssetReady::Icons { page, cells } => {
                self.layer.icons_mut().add(page, &cells);
            }
        }
    }

    /// Shows the gear that opens the settings at the window's top right while the game screen
    /// is shown with no sheet over it, or while the bug report takes its picture, and takes it
    /// away otherwise.
    pub(super) fn show_gear(&mut self) {
        let gear = (self.screen == Screen::Game && self.table_shown()).then(screens::gear);
        let shown = gear.is_some();
        self.layer.float(floats::GEAR, gear);
        if shown {
            self.layer.set_float_look(
                floats::GEAR,
                FloatLook {
                    point: Vec2::new(
                        self.window.size.width - screens::GEAR_INSET,
                        screens::GEAR_INSET,
                    ),
                    anchor: Vec2::new(1.0, 0.0),
                    ..FloatLook::default()
                },
            );
        }
    }
}
