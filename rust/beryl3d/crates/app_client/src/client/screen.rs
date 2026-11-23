//! Showing the flat layer's screen or sheet, and the corner's buttons, a float at the game
//! screen's top right: the gear that opens the settings sheet, the bug button and undo.

use beryl_protocol::DecisionId;
use beryl_protocol::protocol::Status;
use glam::Vec2;

use super::Client;
use super::features::{Features, Placement, ViewInputs};

use crate::command::{Request, SessionCommand};
use app_screens::screens::{self, Screen, ScreenIn, Sheet, bug_report};
use game_hud::hud::HudIn;
use game_hud::hud::inspector::Message as Inspect;
use game_hud::hud::placement_scene::PlacementScene;
use game_hud::hud::rows::Message as RowMessage;
use game_hud::screens::game::Furniture;
use game_manual::features::{context_menu, table::Message as TableMessage};
use game_table::faces::{FaceList, Faces};
use kit_ui::element::{Action, Element, floats};
use kit_ui::input::{AssetReady, FaceId};
use kit_ui::layout::FloatLook;

struct MountedViews {
    sheet: Option<Element>,
    screen: Option<Element>,
    log: Option<Element>,
    result: Option<Element>,
    menu_face: Option<FaceId>,
}

impl Client {
    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    pub(super) fn show_screen(&mut self) {
        self.ask_saved_tables();
        // The element a gesture began on is gone with the screen or sheet it was on.
        let sheet = self.features.sheet;
        if self.features.shown != (self.features.screen, sheet) {
            self.features.shown = (self.features.screen, sheet);
            self.refresh_frame_rates();
            self.cancel_gesture();
            self.routes.scrolling.cancel();
            self.features.focus_row(RowMessage::Focus(None));
            self.cancel_touch();
            self.features.inspect(&Inspect::Pin(None));
        }
        self.features.inspect(&Inspect::Invalidate);
        self.refresh_screen_state();
        let under_visible = sheet != Some(Sheet::Settings) && !self.bug_hides_screen();
        let furniture = self.game_furniture(under_visible);
        let placement_scene = self.placement_scene();
        let MountedViews {
            sheet: sheet_view,
            screen: mounted_screen,
            log,
            result,
            menu_face,
        } = self.mounted_views(under_visible, placement_scene.as_ref());
        let view = self.show.latest().map(|update| &update.view);
        let input = ScreenIn {
            placement_scene: placement_scene.as_ref(),
            ..self.screen_in()
        };
        let mut full = game_hud::prompts::faces(&input.hud());
        full.extend(menu_face);
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
            || (self.features.screen == Screen::Game
                && input.prompt.is_none()
                && input.view.is_none())
        {
            None
        } else {
            mounted_screen.or_else(|| self.compose_screen(&input, furniture, log, result))
        };
        let game_opening =
            self.features.screen == Screen::Game && game_hud::screens::opening::shown(&input.hud());
        // The bug report captures the opening under its sheet. Keep the same life HUD and
        // opening-hand placement while the header is still drawn.
        let opening = under.is_some() && game_opening;
        let shown = match sheet {
            Some(Sheet::Settings) => sheet_view,
            Some(Sheet::BugReport) => Some(bug_report::compose(under, sheet_view)),
            None => under,
        };
        let music = screens::music(self.features.screen, view);
        self.features.opening.shown = opening;
        // While the opening shows the hand, the table's hand waits below the window, as the web's
        // desktop board holds it away under the opening, a sheet open over it or not; the
        // phone's board does not.
        if self.show.table().layout().style.hand_away > 0.0
            && self.show.hold_hand_away(game_opening, self.layout_window())
        {
            self.prepare_resized();
        }
        if let Some(probe) = &mut self.probe {
            probe.attempts = placement_scene
                .as_ref()
                .map_or_else(Vec::new, PlacementScene::take_attempts);
        }
        self.frame.clearances = placement_scene
            .map(PlacementScene::into_clearances)
            .unwrap_or_default();
        let duration = self.data().flat.transition_ms;
        let transitions = if self.probe.is_some() {
            kit_ui::widgets::screen::with_recorded_transitions
        } else {
            kit_ui::widgets::screen::with_transitions
        };
        self.layer
            .show(shown.map(|screen| transitions(screen, duration)));
        self.features.opening.coin_element = (opening || self.bug_open())
            .then(|| {
                self.layer
                    .screen_element(game_hud::screens::opening::coin::NAME)
            })
            .flatten();
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
        if music != self.features.music {
            self.features.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// Updates the opening clock and whether a result can be dismissed.
    fn refresh_screen_state(&mut self) {
        let played = self.show.shown();
        self.features.opening.update(
            played.filter(|_| self.features.screen == Screen::Game),
            self.features.bug().table_time(self.now),
            self.show.table().data().shown.opening.flip_ms,
        );
        // A result put aside comes back with the next game's end.
        if self.show.latest().is_none_or(|update| {
            !matches!(update.view.status, Status::Over { .. } | Status::Stopped)
        }) {
            self.features.reset_result();
        }
    }

    /// Fills the HUD positions in the game screen from the registered views.
    fn game_furniture(&mut self, visible: bool) -> Furniture {
        if !visible || self.features.screen != Screen::Game {
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
            prompt: None,
        }
    }

    /// Adds the mounted prompt to the game screen's furniture and overlays.
    fn compose_screen(
        &self,
        input: &ScreenIn<'_>,
        mut furniture: Furniture,
        log: Option<Element>,
        result: Option<Element>,
    ) -> Option<Element> {
        // The prompt takes presses only after the script of its view has played out.
        furniture.prompt = (input.screen == Screen::Game)
            .then(|| self.features.prompt_view(input.hud(), self.probe.is_some()))
            .flatten()
            .map(Element::waits);
        screens::screen(input, furniture, log, result)
    }

    /// Builds registered views while their inputs are borrowed from the client.
    fn mounted_views(
        &mut self,
        under_visible: bool,
        placement_scene: Option<&PlacementScene>,
    ) -> MountedViews {
        self.with_mounted_inputs(placement_scene, |features, feature_inputs, menu_face| {
            let sheet = feature_inputs.hud.sheet;
            let sheet_view =
                sheet.and_then(|sheet| features.view(Placement::Sheet(sheet), feature_inputs));
            let log = (under_visible && feature_inputs.screen == Screen::Game)
                .then(|| features.view(Placement::GameLog, feature_inputs))
                .flatten();
            let mounted_screen = under_visible
                .then(|| features.view(Placement::Screen(feature_inputs.screen), feature_inputs))
                .flatten();
            let result = (under_visible && feature_inputs.screen == Screen::Game)
                .then(|| features.view(Placement::GameOver, feature_inputs))
                .flatten();
            MountedViews {
                sheet: sheet_view,
                screen: mounted_screen,
                log,
                result,
                menu_face,
            }
        })
    }

    /// Builds a registered feature's view using the same state and inputs as its screen.
    pub(crate) fn feature_element(&mut self, id: &str) -> Option<Element> {
        let placement = self.placement_scene();
        self.with_mounted_inputs(placement.as_ref(), |features, inputs, _| {
            features.view_named(id, inputs)
        })
    }

    #[cfg(feature = "stories")]
    pub(crate) fn settings_story(&self) -> game_probe::story::SettingsStory {
        use crate::settings::Graphics;

        let settings = self.settings.screen();
        game_probe::story::SettingsStory {
            scale: Some(self.window.scale),
            screen: Some(self.features.screen.name().to_owned()),
            confirm_concede: self.features.settings_state().confirm_concede,
            game_over: self
                .show
                .latest()
                .is_some_and(|update| matches!(update.view.status, Status::Over { .. })),
            music: Some(settings.music),
            effects: Some(settings.effects),
            muted: Some(settings.muted),
            full_control: Some(settings.full_control),
            fps_monitor: Some(settings.fps_monitor),
            graphics: Some(
                match settings.graphics {
                    Graphics::Automatic => "automatic",
                    Graphics::Baseline => "baseline",
                    Graphics::Enhanced => "enhanced",
                }
                .to_owned(),
            ),
        }
    }

    fn with_mounted_inputs<T>(
        &mut self,
        placement_scene: Option<&PlacementScene>,
        build: impl FnOnce(&mut Features, ViewInputs<'_>, Option<FaceId>) -> T,
    ) -> T {
        let sheet = self.features.sheet;
        let window = self.layout_window();
        let view = self.show.latest().map(|update| &update.view);
        let played = self.show.shown().map(|update| &update.view);
        let profile = self.show.table().profile();
        let revealed = if self.features.screen == Screen::Table {
            self.features.viewer().revealed().to_vec()
        } else {
            Vec::new()
        };
        let hud = HudIn {
            data: self.show.table().data(),
            window,
            profile,
            view,
            played,
            turn: self.features.steps().turn,
            prompt: None,
            stops: beryl_protocol::StopsSetting {
                full_control: self.settings.full_control,
                ..self.settings.stops
            },
            sheet,
            faces: Some(self.show.faces().list()),
            viewing: self.features.viewer().open(),
            revealed: &revealed,
            carried: self.carried_item(),
            spinning: self.opening_spinning(),
            placement_scene,
            ways_tray: None,
        };
        let menu_face = context_menu::sheet_face(self.features.menu(), &hud);
        let feature_inputs = ViewInputs {
            explaining: self.probe.is_some(),
            drag_threshold: self.data().pointer.drag_threshold_points,
            profile,
            screen: self.features.screen,
            window,
            view,
            session: self.session,
            fields: self.layer.fields(),
            layer: &self.layer,
            hud: &hud,
            bindings: &self.settings.bindings,
            faces: self.show.faces().list(),
            game_over: view.is_some_and(|view| matches!(view.status, Status::Over { .. })),
            settings: self.settings.screen(),
            start: None,
            link_error: None,
        };
        build(&mut self.features, feature_inputs, menu_face)
    }

    /// Everything the screen functions read, from what the client holds now.
    pub(super) fn screen_in(&self) -> ScreenIn<'_> {
        ScreenIn {
            data: self.show.table().data(),
            screen: self.features.screen,
            window: self.layout_window(),
            profile: self.show.table().profile(),
            view: self.show.latest().map(|update| &update.view),
            played: self.show.shown().map(|update| &update.view),
            turn: self.features.steps().turn,
            // When the opponent won, the first spin holds back the result and any new decision.
            prompt: self
                .features
                .prompts
                .current()
                .filter(|_| !self.opening_spinning()),
            full_control: self.settings.full_control,
            stops: self.settings.stops,
            sheet: self.features.sheet,
            fields: self.layer.fields(),
            faces: Some(self.show.faces().list()),
            viewing: self.features.viewer().open(),
            revealed: self.features.viewer().revealed(),
            carried: self.carried_item(),
            spinning: self.opening_spinning(),
            placement_scene: None,
        }
    }

    fn opening_spinning(&self) -> bool {
        self.features
            .opening
            .spinning(self.features.bug().table_time(self.now))
    }

    /// The active scene's faces, which the host's asset workers prepare.
    #[must_use]
    pub fn faces(&self) -> &FaceList {
        self.face_layers().list()
    }

    /// The layer each active face has landed in at each size.
    #[must_use]
    pub fn face_layers(&self) -> &Faces {
        if self.active_story_faces() {
            return self
                .stories
                .board()
                .expect("active story board")
                .face_layers();
        }
        self.show.faces()
    }

    /// Identifies which scene owns the renderer's card face layers.
    #[must_use]
    pub fn face_generation(&self) -> (bool, u64) {
        if self.active_story_faces() {
            (true, self.stories.generation)
        } else {
            (false, self.show.faces().number())
        }
    }

    pub(crate) fn active_story_faces(&self) -> bool {
        self.features.screen == Screen::Gallery && self.stories.board().is_some()
    }

    /// How many glyphs the flat layer's text asked for that no atlas page holds yet
    /// (`Glyphs::missing`).
    #[must_use]
    pub fn missing_glyphs(&self) -> usize {
        self.layer.glyphs().missing()
    }

    /// The host rebuilt the face arrays. Full-size faces need uploading again.
    pub fn face_arrays_made(&mut self) {
        if self.active_story_faces() {
            self.stories
                .board_mut()
                .expect("active story board")
                .face_arrays_made();
        } else {
            self.show.faces_mut().low_memory();
        }
    }

    /// Records where the host uploaded each face, glyph or icon the client can draw.
    pub(super) fn asset_ready(&mut self, asset: AssetReady) {
        self.stories.asset(&asset);
        match asset {
            asset @ AssetReady::Face { .. } if self.active_story_faces() => {
                self.stories.give(asset);
            }
            AssetReady::Face { face, size, layer } => {
                self.show.faces_mut().arrived(face, size, layer);
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
    /// take back. Settings and undo stay visible under the bug report.
    pub(super) fn show_gear(&mut self) {
        // The phone's menu button, part of its game screen, opens the settings instead.
        // The game screen's undo takes back an engine decision; the manual table's runs its
        // `table.undo` command, as the web's `Table` passes it to `GameBoard`.
        let undo = match self.features.screen {
            Screen::Table => game_manual::manual::commands::command("table.undo")
                .map(|(index, _)| Action::new(TableMessage::Shortcut(index, false))),
            _ => self
                .undoable()
                .map(|_| Action::new(game_hud::prompts::bar::Message::Undo)),
        };
        let bug = !self.bug_open();
        let profile = self.show.table().profile();
        let at = match self.features.screen {
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
        (decision.undo > 0 && !self.features.prompts.waiting_for_reply()).then_some(decision.id)
    }

    /// Takes back the viewer's latest answer, one for each press of undo or Cmd+Z, across the
    /// opponent's turns: the session undoes it and sends the views from there.
    pub(super) fn undo(&mut self) {
        if let Some(decision) = self.undoable() {
            self.commands.push_back(SessionCommand::Undo(decision));
        }
    }
}
