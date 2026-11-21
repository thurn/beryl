//! Showing the flat layer's screen or sheet, and the corner's buttons, a float at the game
//! screen's top right: the gear that opens the settings sheet, the bug button and undo.

use beryl_protocol::DecisionId;
use beryl_protocol::protocol::Status;
use glam::Vec2;

use super::Client;
use super::features::{Placement, ViewInputs};

use crate::command::{Request, SessionCommand};
use app_screens::screens::{self, Screen, ScreenIn, ScreenPress, Sheet, bug_report};
use game_hud::hud::inspector::Message as Inspect;
use game_hud::hud::placement_scene::PlacementScene;
use game_hud::hud::rows::Message as RowMessage;
use game_hud::screens::game::Furniture;
use game_manual::manual::TableCommand;
use game_table::faces::{FaceList, Faces};
use kit_ui::element::{Action, floats};
use kit_ui::input::{AssetReady, FaceSize};
use kit_ui::layout::FloatLook;

impl Client {
    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    pub(super) fn show_screen(&mut self) {
        self.ask_saved_tables();
        // The element a gesture began on is gone with the screen or sheet it was on.
        let sheet = self.features.sheet;
        if self.shown != (self.screen, sheet) {
            self.shown = (self.screen, sheet);
            self.end_gesture();
            self.scrolling.cancel();
            self.features.rows_mut().update(RowMessage::Focus(None));
            self.cancel_touch();
            self.features.inspector_mut().update(Inspect::Pin(None));
        }
        self.features.inspector_mut().update(Inspect::Invalidate);
        self.refresh_screen_state();
        let under_visible = sheet != Some(Sheet::Settings) && !self.bug_hides_screen();
        let furniture = self.game_furniture(under_visible);
        let view = self.show.latest().map(|update| &update.view);
        let feature_inputs = ViewInputs {
            profile: self.show.table().profile(),
            screen: self.screen,
            window: self.window,
            view,
            session: self.session,
            fields: self.layer.fields(),
            entries: &self.manual.entries,
            faces: self.show.faces().list(),
            game_over: view.is_some_and(|view| matches!(view.status, Status::Over { .. })),
            settings: self.settings.screen(),
        };
        let sheet_view =
            sheet.and_then(|sheet| self.features.view(Placement::Sheet(sheet), feature_inputs));
        let log = (under_visible && self.screen == Screen::Game)
            .then(|| self.features.view(Placement::GameLog, feature_inputs))
            .flatten();
        let mounted_screen = under_visible
            .then(|| {
                self.features
                    .view(Placement::Screen(self.screen), feature_inputs)
            })
            .flatten();
        let result = (under_visible && self.screen == Screen::Game)
            .then(|| self.features.view(Placement::GameOver, feature_inputs))
            .flatten();
        let placement_scene = self.placement_scene();
        let input = ScreenIn {
            placement_scene: placement_scene.as_ref(),
            ..self.screen_in()
        };
        let mut full = game_hud::prompts::faces(&input.hud());
        full.extend(screens::sheet_face(&input));
        // On the desktop the hand and the stack show cards' full frames, as the web interface
        // does; on the phone the battlefield frame.
        let read_whole = view
            .filter(|_| input.profile.data().table_style.full_card_faces)
            .map(|view| self.show.faces().list().read_whole(view));
        // The settings sheet takes the screen's place in the flat
        // layer; the bug report is drawn over the screen. Before a game has a view and with no
        // prompt open, the game screen shows nothing, so that no empty panel is drawn over the
        // table. Once a game has a view it shows the players' furniture, and once the game has
        // ended, the result.
        let under = if !under_visible
            || (self.screen == Screen::Game && input.prompt.is_none() && input.view.is_none())
        {
            None
        } else {
            mounted_screen.or_else(|| screens::screen(&input, furniture, log, result))
        };
        let opening = under.is_some()
            && sheet.is_none()
            && self.screen == Screen::Game
            && game_hud::screens::opening::shown(&input.hud());
        let away = self.screen == Screen::Game && game_hud::screens::opening::shown(&input.hud());
        let shown = match sheet {
            Some(Sheet::Settings) => sheet_view,
            Some(Sheet::BugReport) => Some(bug_report::compose(under, sheet_view)),
            None => under,
        };
        let music = screens::music(self.screen, view);
        self.opening.shown = opening;
        // While the opening shows the hand, the table's hand waits below the window, as the web's
        // desktop board holds it away under the opening, a sheet open over it or not; the
        // phone's board does not.
        if self.show.table().layout().style.hand_away > 0.0
            && self.show.hold_hand_away(away, self.window)
        {
            self.prepare_resized();
        }
        self.frame.clearances = placement_scene
            .map(PlacementScene::into_clearances)
            .unwrap_or_default();
        self.layer
            .show(shown.map(kit_ui::widgets::screen::with_transitions));
        if let Some(field) = self.features.focus_after_show.take() {
            self.layer.focus_field(field);
        }
        self.layer.scroll_to_end(game_hud::hud::log::LIST);
        self.place_shown();
        let read_whole = read_whole
            .iter()
            .flat_map(|faces| faces.faces().iter().copied());
        for face in full.into_iter().chain(read_whole) {
            self.show.faces_mut().want_full(face);
        }
        self.show_gear();
        if music != self.music {
            self.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// Updates the opening clock and whether a result can be dismissed.
    fn refresh_screen_state(&mut self) {
        let played = self.show.shown().map(|update| &update.view);
        self.opening
            .spin(played.filter(|_| self.screen == Screen::Game), self.now);
        // A result put aside comes back with the next game's end.
        if self.show.latest().is_none_or(|update| {
            !matches!(update.view.status, Status::Over { .. } | Status::Stopped)
        }) {
            self.features.reset_result();
        }
    }

    /// Fills the HUD positions in the game screen from the registered views.
    fn game_furniture(&mut self, visible: bool) -> Furniture {
        if !visible || self.screen != Screen::Game {
            return Furniture::default();
        }
        let input = self.screen_in().hud();
        let steps = game_hud::hud::steps::Inputs::new(&input);
        let dock = game_hud::hud::full_control::Inputs::dock(&input);
        let band = game_hud::hud::full_control::Inputs::middle_band(&input);
        let players = input
            .profile
            .data()
            .hud
            .table
            .contains(&kit_base::data::profiles::Piece::Players)
            .then(|| game_hud::hud::players::players(&input))
            .flatten();
        let viewer = game_hud::hud::viewer::Inputs::new(
            self.show.table().profile(),
            self.window.size,
            self.show.latest().map(|update| &update.view),
            self.views,
        );
        Furniture {
            players,
            steps: self.features.step_view(steps),
            dock_full_control: dock.map(|input| self.features.full_control_view(input)),
            band_full_control: band.map(|input| self.features.full_control_view(input)),
            viewer: viewer.and_then(|input| self.features.viewer_view(input)),
        }
    }

    /// Everything the screen functions read, from what the client holds now. The inspector sets
    /// `inspected` on it; nothing else differs between the screen and the inspector.
    pub(super) fn screen_in(&self) -> ScreenIn<'_> {
        ScreenIn {
            screen: self.screen,
            window: self.window,
            profile: self.show.table().profile(),
            view: self.show.latest().map(|update| &update.view),
            played: self.show.shown().map(|update| &update.view),
            turn: self.features.steps().turn,
            // While the opening's coin spins, no decision is offered, as the web's `Opening` offers
            // none.
            prompt: (self.prompt.as_ref())
                .map(|(_, prompt)| prompt)
                .filter(|_| !self.opening.spinning(self.now)),
            full_control: self.settings.full_control,
            stops: self.settings.stops,
            bindings: &self.settings.bindings,
            sheet: self.features.sheet,
            gallery: self.gallery,
            fields: self.layer.fields(),
            inspected: None,
            faces: Some(self.faces()),
            viewing: self.features.viewer().open(),
            revealed: self.features.viewer().revealed(),
            table: &self.manual.state,
            carried: self.gesture.carried(),
            lobby: &self.manual.lobby,
            spinning: self.opening.spinning(self.now),
            placement_scene: None,
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

    /// How many glyphs the flat layer's text asked for that no atlas page holds yet
    /// (`Glyphs::missing`).
    #[must_use]
    pub fn missing_glyphs(&self) -> usize {
        self.layer.glyphs().missing()
    }

    /// The host made its face arrays anew, so that the full-size layers the match's faces were
    /// given are empty: the match's faces forget them and are asked for again when read, as
    /// after low memory. Their board-size layers stay, since the host uploads every board-size
    /// face again into the layer numbered as its `FaceId`.
    pub fn face_arrays_made(&mut self) {
        self.show.faces_mut().low_memory();
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
                self.show.badges_mut().add_icons(page, &cells);
            }
        }
    }

    /// Shows the corner's buttons at the window's top right (`screens::corner`): the gear that
    /// opens the settings, the bug button and undo while the open decision has an answer to
    /// take back. They show while the game screen is shown with no sheet over it, or while the
    /// bug report takes its picture, and go otherwise.
    pub(super) fn show_gear(&mut self) {
        // The phone's menu button, part of its game screen, opens the settings instead.
        // The game screen's undo takes back an engine decision; the manual table's runs its
        // `table.undo` command, as the web's `Table` passes it to `GameBoard`.
        let undo = match self.screen {
            Screen::Table => game_manual::manual::commands::command("table.undo")
                .map(|(index, _)| Action::new(TableCommand(index, false))),
            _ => self.undoable().map(|_| Action::new(ScreenPress::Undo)),
        };
        let bug = !self.bug_open();
        let profile = self.show.table().profile();
        let at = match self.screen {
            Screen::Game
                if profile
                    .data()
                    .hud
                    .table
                    .contains(&kit_base::data::profiles::Piece::Gear) =>
            {
                profile
                    .slot("gear")
                    .map(|slot| Vec2::new(slot.rect.x + slot.rect.width, slot.rect.y))
            }
            Screen::Table if profile.size_class() == kit_scene::SizeClass::Regular => {
                Some(Vec2::new(
                    self.window.size.width - screens::GEAR_INSET,
                    screens::GEAR_INSET,
                ))
            }
            _ => None,
        }
        .filter(|_| self.table_shown());
        self.layer
            .float(floats::GEAR, at.map(|_| screens::corner(bug, undo)));
        if let Some(point) = at {
            self.layer.set_float_look(
                floats::GEAR,
                FloatLook {
                    point,
                    anchor: Vec2::new(1.0, 0.0),
                    ..FloatLook::default()
                },
            );
        }
    }

    /// The open decision while one of the viewer's answers can be taken back from it and no
    /// click is waiting for its answer, as the web's `StageBoard` shows its undo button.
    pub(super) fn undoable(&self) -> Option<DecisionId> {
        let decision = self.open_decision()?;
        (decision.undo > 0 && self.clicks.is_empty()).then_some(decision.id)
    }

    /// Takes back the viewer's latest answer, one for each press of undo or Cmd+Z, across the
    /// opponent's turns: the session undoes it and sends the views from there.
    pub(super) fn undo(&mut self) {
        if let Some(decision) = self.undoable() {
            self.commands.push_back(SessionCommand::Undo(decision));
        }
    }
}
