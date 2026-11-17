//! Showing the flat layer's screen or sheet, and the corner's buttons, a float at the game
//! screen's top right: the gear that opens the settings sheet, the bug button and undo.

use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::{AgentView, DecisionId};
use glam::Vec2;

use super::Client;

use crate::command::{Request, SessionCommand};
use crate::element::floats;
use crate::faces::{FaceList, Faces};
use crate::hud::phone::Occupied;
use crate::input::Rect;
use crate::input::{AssetReady, FaceSize};
use crate::layout::FloatLook;
use crate::screens::{self, Screen, ScreenIn, Sheet, StartForm};

impl Client {
    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    pub(super) fn show_screen(&mut self) {
        self.ask_saved_tables();
        // The element a gesture began on is gone with the screen or sheet it was on.
        if self.shown != (self.screen, self.sheet) {
            self.shown = (self.screen, self.sheet);
            self.end_gesture();
            self.cancel_touch();
            self.inspector.pinned = None;
        }
        self.inspector.due = true;
        self.log.folded = self.permanent_over_log() || self.pool_over_log();
        let view = self.show.latest().map(|update| &update.view);
        let occupied = self.occupied();
        let input = ScreenIn {
            occupied: &occupied,
            ..self.screen_in()
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
        let opening = under.is_some()
            && self.sheet.is_none()
            && self.screen == Screen::Game
            && screens::opening::shown(&input);
        let shown = match self.sheet {
            Some(sheet) => Some(screens::sheet(sheet, under, &input)),
            None => under,
        };
        let music = screens::music(self.screen, view);
        self.viewing.opening = opening;
        self.layer.show(shown);
        self.layer.scroll_to_end(crate::hud::log::LIST);
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

    /// Keeps the agents a person may play and chooses the first to offer. The live app sends
    /// them once it has resumed a game or found none to resume, so agents that arrive before
    /// any view mean there is no game yet: the person starts one on the start screen.
    pub(super) fn take_agents(&mut self, agents: Vec<AgentView>) {
        if self.views == 0 {
            self.screen = Screen::Start;
            self.first_view_opens = Some(Screen::Game);
        }
        self.start.agent = StartForm::first_agent(&agents);
        self.agents = agents;
        self.show_screen();
    }

    /// Everything the screen functions read, from what the client holds now. The inspector sets
    /// `inspected` on it; nothing else differs between the screen and the inspector.
    pub(super) fn screen_in(&self) -> ScreenIn<'_> {
        ScreenIn {
            screen: self.screen,
            window: self.window,
            view: self.show.latest().map(|update| &update.view),
            prompt: self.prompt.as_ref().map(|(_, prompt)| prompt),
            settings: &self.settings,
            sheet: self.sheet,
            agents: &self.agents,
            start: &self.start,
            entries: &self.manual.entries,
            gallery: self.gallery,
            recordings: &self.recordings,
            seat: self.seat,
            bug: &self.bug.form,
            fields: &self.fields,
            inspected: None,
            faces: Some(self.faces()),
            viewing: self.viewing.open,
            revealed: &self.viewing.revealed,
            table: &self.manual.state,
            carried: self.gesture.carried(),
            lobby: &self.manual.lobby,
            log: self.log.logged.entries(),
            log_folded: self.log.folded,
            log_open: self.log.open,
            concede: self.concede,
            occupied: &crate::hud::phone::NOTHING,
        }
    }

    /// Opens the game log or closes it. The phone's settings sheet holds the log's button, so a
    /// press there also closes the sheet, which would otherwise take the screen's place.
    pub(super) fn toggle_log(&mut self) {
        self.log.open = !self.log.open;
        if self.sheet.is_some() {
            self.set_sheet(None);
        } else {
            self.show_screen();
        }
    }

    /// Where the latest view's cards will rest once every script given so far has played, on
    /// the phone layout, whose prompt pill and dock keep clear of them; nothing on the desktop.
    fn occupied(&self) -> Occupied {
        let Some(update) = self.show.latest().filter(|_| self.window.phone()) else {
            return Occupied::default();
        };
        let camera = self.show.table().camera();
        let mut occupied = Occupied::default();
        for object in &update.view.objects {
            let Some(pose) = self.show.latest_rest(usize::from(object.slot.0)) else {
                continue;
            };
            let corners = camera.card_box(&pose).0;
            let (mut low, mut high) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for corner in corners {
                low = low.min(corner);
                high = high.max(corner);
            }
            match object.zone {
                ZoneName::Battlefield | ZoneName::Stack => occupied.cards.push(Rect {
                    x: low.x,
                    y: low.y,
                    width: high.x - low.x,
                    height: high.y - low.y,
                }),
                ZoneName::Hand if object.owner == Side::You => {
                    occupied.hand_top = Some(occupied.hand_top.map_or(low.y, |top| top.min(low.y)));
                }
                _ => {}
            }
        }
        occupied
    }

    /// Whether a permanent of the latest view, where it will rest once every script given so far
    /// has played, reaches into the game log's column (`hud::log::reaches_column`).
    fn permanent_over_log(&self) -> bool {
        let Some(update) = self.show.latest() else {
            return false;
        };
        let camera = self.show.table().camera();
        (update.view.battlefield.iter()).any(|slot| {
            self.show
                .latest_rest(usize::from(slot.0))
                .is_some_and(|pose| crate::hud::log::reaches_column(&camera.card_box(&pose).0))
        })
    }

    /// A nonempty pool whose first mana symbol stands in the log's column folds the log,
    /// just as a permanent there does. The pool must stay readable while the player pays.
    fn pool_over_log(&self) -> bool {
        let Some(update) = self.show.latest() else {
            return false;
        };
        let layout = self.show.table().layout();
        [&update.view.players.you, &update.view.players.opponent]
            .into_iter()
            .zip(&layout.players)
            .any(|(player, anchor)| {
                let point = crate::hud::counts::points(anchor.pool, self.window.size);
                let size = crate::tokens::type_style::POOL.size;
                player.mana != beryl_protocol::protocol::ManaPool::default()
                    && crate::hud::log::reaches_column(&[
                        point,
                        point + Vec2::new(size, 0.0),
                        point + Vec2::splat(size),
                        point + Vec2::new(0.0, size),
                    ])
            })
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
        let (bug, undo) = (!self.bug_open(), self.undoable().is_some());
        let gear = (self.screen == Screen::Game && self.table_shown() && !self.window.phone())
            .then(|| screens::corner(bug, undo));
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
