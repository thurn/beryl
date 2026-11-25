//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "The frame" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

mod bug;
mod carry;
mod data;
mod editing;
mod event;
mod explain;
pub(crate) use crate::command::{Reply, Request, SessionCommand};
pub use event::Input;
mod card_input;
mod features;
pub use features::{FeatureInfo, composed_features, registered_features};

use kit_ui::input::Input as DeviceInput;
use std::collections::VecDeque;

use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Click;
use beryl_protocol::protocol::SessionId;
use beryl_protocol::script::{OwnTimings, Verb, own_timings};

use app_screens::screens::{Screen, bug_report, start, testing};
use game_hud::hud::banner;
use game_hud::hud::{counts, notices, piles, rows};
use game_hud::prompts;
use game_manual::features::table::RefusalSound;
use game_play::play::Show;
use game_play::play::record::{Plant, ScriptRecord};
use game_table::faces::{CARD_BACK, FaceList};
use game_table::table::{Table, Window};
use kit_base::data::Data;
use kit_frame::frame::{self, Frame, FrameBuilder, Sound, Wake};
use kit_scene::Tree;
use kit_ui::input::{FaceId, FaceSize, GlyphKey, Lifecycle, Tier};
use kit_ui::layout::{self, Layer};
use std::sync::Arc;

mod actions;
mod input;
mod inspect;
mod lobby;
mod manual;
mod manual_overlays;
mod own;
mod performance;
mod profiles;
mod refusal;
mod screen;
mod scrolling;
mod stops;
mod story;
mod viewing;

/// The platform a host runs on, as far as the client cares: where `Cmd` in a shortcut means
/// the Command key, and where it means Control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Mac,
    Other,
}

/// The sizes the client sets at startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The most resolved elements a frame holds.
    pub elements: usize,
    /// The most draw records a frame holds.
    pub draws: usize,
    /// The most bytes of element names a frame holds.
    pub name_bytes: usize,
}

impl Config {
    /// Sizes for the engine's largest board, so that no frame allocates (see `frame::DRAWS`).
    pub const LARGEST: Self = Self {
        elements: frame::ELEMENTS,
        draws: frame::DRAWS,
        name_bytes: frame::NAME_BYTES,
    };
}

/// The client core.
pub struct Client {
    frame: Frame,
    probe: Option<game_probe::explain::Probe>,
    stories: story::Stories,
    /// The host's window, with its layout class resolved.
    window: Window,
    /// The table and the scripts that lead it from view to view.
    show: Show,
    /// The flat layer: the screen, the sheet over it and the floats.
    layer: Layer,
    /// The registered features, their state and the open sheet route.
    features: features::Features,
    routes: input::Routes,
    settings: crate::settings::Settings,
    /// The session the latest view came from, which stop and concede commands name.
    session: Option<SessionId>,
    /// Timing shared by prompts and other steps the client plays itself.
    step_timings: OwnTimings,
    /// How many session views the client has been given.
    views: usize,
    /// The host's platform, which says what `Cmd` in a shortcut means.
    platform: Platform,
    /// The `now` of the latest frame: the time of every input since, which has none of its own.
    now: u64,
    wake: Wake,
    requests: VecDeque<Request>,
    commands: VecDeque<SessionCommand>,
    tier: Tier,
    /// Whether the host has said memory is low (`Lifecycle::LowMemory`), which fixes the tier at
    /// Baseline.
    low_memory: bool,
}

impl Client {
    #[must_use]
    pub fn new(data: Arc<Data>, config: Config) -> Self {
        let window = data.profiles.resolve_window(Window::default());
        let layer = Layer::with_icons(data.flat, Some(game_hud::mana_icons::icon));
        let mut frame = Frame::with_capacity(config.elements, config.draws, config.name_bytes);
        frame.labels = &game_hud::labels::GAME_LABELS;
        let table = Table::new(data, window);
        Self {
            frame,
            probe: None,
            stories: story::Stories::default(),
            window,
            show: Show::new(table),
            layer,
            features: features::Features::default(),
            routes: input::Routes::default(),
            settings: crate::settings::Settings::default(),
            session: None,
            step_timings: own_timings(),
            views: 0,
            platform: Platform::Mac,
            now: 0,
            wake: Wake::Never,
            requests: VecDeque::new(),
            commands: VecDeque::new(),
            tier: Tier::Baseline,
            low_memory: false,
        }
    }

    /// The tuning data supplied by the host.
    pub fn data(&self) -> &Arc<Data> {
        self.show.table().data()
    }

    /// The visual tier frames are drawn in. It starts at Baseline (docs/beryl-3d.md, "Choosing a
    /// tier"), follows `Input::Tier`, and stays Baseline for the rest of the session once memory
    /// has run low.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// Takes a session update, a pointer or key event, a new surface or another input. An
    /// input has no time of its own: what it starts begins at the `now` of the next `frame`.
    /// This is where screens are rebuilt and laid out, and where text is shaped.
    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Data(data) => self.set_data(data),
            Input::DataError(_) => self.show_data_error(),
            Input::Update(update) => self.take_game_update(update),
            Input::Reply(Reply::CardSearch(text, hits)) => self.take_token_hits(text, hits),
            Input::Reply(Reply::TableAct(reply)) => self.take_table_reply(reply),
            Input::Reply(Reply::Click(Ok(reply))) => {
                if self
                    .features
                    .prompts
                    .reply(reply, self.views, &mut self.show)
                {
                    self.prepare_counts();
                    self.show_screen();
                }
            }
            Input::Reply(Reply::Click(Err(_))) => {
                self.features.prompts.rejected(&mut self.show);
            }
            Input::Lobby(update) => self.take_lobby(*update),
            Input::Reply(Reply::TableLobby(lobby)) => self.take_lobby_reply(lobby),
            Input::Reply(reply @ (Reply::TableOpen(_) | Reply::TableSaved(_))) => {
                self.take_entry_reply(reply);
            }
            Input::TableError(reason) => self.table_error(reason),
            Input::Reply(Reply::DeckList(saved)) => self.take_saved_decks(saved),
            Input::Reply(Reply::DeckImport(imported)) => self.take_imported_deck(imported),
            Input::Settings(settings) => {
                let monitor_changed = self.settings.fps_monitor != settings.fps_monitor;
                self.settings = settings;
                if monitor_changed {
                    self.apply_frame_monitor_setting();
                }
                self.send_stops();
                self.show_screen();
            }
            Input::Agents(agents) => {
                self.update_start(&start::Message::Agents(agents, self.views == 0));
                self.show_screen();
            }
            Input::Recordings(recordings) => {
                self.update_testing(&testing::Message::Recordings(recordings));
                self.show_screen();
            }
            Input::Stories(sources) => self.set_stories(sources),
            Input::BugSaved(result) => {
                if self.update_bug(&bug_report::Message::Saved(result)) {
                    self.show_screen();
                }
            }
            Input::Device(event) => self.handle_device(event),
            Input::Cards(records) => {
                let list = FaceList::from_records(&records);
                self.show.faces_mut().replace(list);
            }
            Input::Reply(_) | Input::Nudge => {}
        }
        self.follow_entry();
        self.wake = Wake::Now;
    }

    fn handle_device(&mut self, event: DeviceInput) {
        match event {
            DeviceInput::Pointer(pointer) => self.point(pointer),
            DeviceInput::Key(key) => self.key(key),
            DeviceInput::Platform { mac } => {
                self.platform = if mac { Platform::Mac } else { Platform::Other };
            }
            DeviceInput::Assets(asset) => self.asset_ready(asset),
            DeviceInput::Surface {
                size,
                scale,
                safe_area,
            } => {
                self.cancel_touch();
                self.resize(Window {
                    size_class: None,
                    size,
                    scale,
                    safe_area,
                });
            }
            DeviceInput::Text(_) | DeviceInput::Paste(_) => self.type_into(&event),
            DeviceInput::Tier(tier) => self.set_tier(tier),
            DeviceInput::Lifecycle(Lifecycle::LowMemory) => self.low_memory(),
            DeviceInput::Lifecycle(_) => {}
        }
    }

    /// Applies a session update and lays out what its next frames will draw.
    fn take_game_update(&mut self, update: Box<GameUpdate>) {
        // Session numbers only grow, so a newer one is a game the client has not shown yet.
        if self.session.is_none_or(|shown| update.session > shown) {
            self.update_start(&start::Message::NewSession);
        }
        self.views += 1;
        self.take_session(&update);
        if self.features.screen == Screen::Game && self.features.sheet.is_none() {
            // Reconciled buttons can keep their IDs when a new decision repeats their labels.
            let next = match &update.view.status {
                beryl_protocol::protocol::Status::Deciding { decision } => Some(decision.id),
                _ => None,
            };
            if self.features.prompts.decision() != next {
                self.cancel_interaction();
            }
        }
        self.features
            .prompts
            .take_view(&update, self.views, &mut self.show);
        self.prepare_pill(&update.view);
        self.receive_stops(&update);
        // The same view sent again says nothing new happened.
        let again = self
            .show
            .latest()
            .is_some_and(|latest| latest.view == update.view);
        if !again {
            self.features.log_history.take(&update.view);
        }
        self.features.banner(banner::Prepare {
            layer: &mut self.layer,
            view: &update.view,
        });
        self.show.push_update(update);
        // Every life total and pool the scripts to come will show is laid out now, so
        // that a frame lays out none of them.
        self.prepare_counts();
        // And so is every notice they will show.
        self.prepare_notices();
        let (text, glyphs) = self.layer.text_and_glyphs();
        self.show.shape_badges(text, glyphs, self.window.scale);
        let shown = self.show.shown().map(|update| &update.view);
        let coming = self.show.coming_counts().map(|(_, view)| view);
        let table = self.show.table();
        self.features.update_piles(piles::Prepare {
            layer: &mut self.layer,
            table,
            views: shown.into_iter().chain(coming),
        });
        self.prepare_rows();
        self.show_screen();
    }

    /// Takes the tier the host's rate rule chose, unless memory has run low, after which the
    /// tier stays Baseline (docs/beryl-3d.md, "Choosing a tier").
    fn set_tier(&mut self, tier: Tier) {
        if !self.low_memory && self.tier != tier {
            self.tier = tier;
            // The FPS monitor names the tier.
            self.refresh_frame_rates();
        }
    }

    /// Memory is low (docs/beryl-3d.md, "Hosts": "Low memory"): forgets every full-size face,
    /// which the renderer has freed, and draws in the Baseline tier for the rest of the session.
    fn low_memory(&mut self) {
        self.low_memory = true;
        if self.tier != Tier::Baseline {
            self.tier = Tier::Baseline;
            self.refresh_frame_rates();
        }
        self.show.faces_mut().low_memory();
    }

    /// Whether the hud's floats (the life totals, pools, pile counts and notices) show: on the
    /// game screen and the manual table, including under the bug report (`table_shown`).
    fn hud_shown(&self) -> bool {
        matches!(self.features.screen, Screen::Game | Screen::Table) && self.table_shown()
    }

    /// Whether each frame records how its flat-layer elements look and the words of each line
    /// of text (`Frame::looking`), for the frame inspector.
    pub fn set_looking(&mut self, looking: bool) {
        self.frame.looking = looking;
    }

    /// The frame at `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame {
        self.now = now;
        self.features.begin_frame(now);
        // The table plays on its own clock, which the bug report stops (`bug`).
        let table_now = self.bug_frame(now);
        self.prepare_table(table_now);
        let reporting = self.bug_open();
        self.advance_gesture(now);
        self.advance_prompt(table_now);
        self.table_frame(table_now);
        let covers = self.prompt_covers_table();
        self.show.cover_table(covers);
        let targets = self.features.prompts.targets(self.table_open());
        let transitioning = self.tick_layer(now)
            | self.shake_button(table_now)
            | self.routes.scrolling.moving(now, &self.data().pointer);
        self.show.advance_pointer(table_now);
        self.show.advance_slide(table_now);
        self.inspect(table_now);
        self.viewing_frame(table_now);
        self.pose_opening_coin(table_now);
        self.layer
            .visit_shows(|placement| self.show.update_show(placement));
        let visible = self.hud_shown();
        // The guided start's opening covers the table's life totals and pile counts, not notices.
        let counted = visible && !self.guided_start();
        let rows_counted = self.rows_counted(visible, covers);
        let manual = self.features.screen == Screen::Table;
        let card = self.data().card.size();
        let mut builder = FrameBuilder::new(&mut self.frame, self.tier, card);
        // The flat layer is placed for the table's camera, so the frame takes it even when no
        // game view writes the table, as on the start screen (the "layer drawn" check).
        builder.camera(self.show.table().camera().view_projection);
        builder.sounds(self.features.update_table(RefusalSound));
        self.show.frame(table_now, &mut builder);
        // Elements anchored to a card, an ability or a player stand where the table drew it.
        let table = self.show.table();
        table.write_hud_anchors(&mut builder, visible);
        if manual {
            table.write_drop_regions(&mut builder);
        }
        self.layer
            .follow(|subject| game_hud::hud::anchors::spot_of(&builder, subject));
        update_hud_features(
            &mut self.features,
            &mut self.layer,
            &self.show,
            (table_now, self.tier),
            (visible, counted, rows_counted, covers),
            targets,
        );
        Tree::solve_float_anchors(&mut self.layer, &builder);
        let (layout_nodes, text_runs) = self.layer.take_work();
        builder.layout_work(layout_nodes, text_runs);
        builder.placement_work(self.show.table().take_placed(), self.show.take_compiled());
        // The bug report is drawn whether or not the table has settled (`bug`).
        self.features.layer_open = reporting || self.show.settled();
        let faces = self.show.faces();
        // While scripts play only what waits for the table (`Element::waits`) is left out.
        let face_layers = |face, size| match size {
            FaceSize::Board => faces.board_layer_of(face),
            FaceSize::Full => faces.full_layer_of(face),
        };
        let layers = layout::FaceLayers {
            back: CARD_BACK,
            layer: &face_layers,
        };
        let table = self.show.table();
        let plane = table.camera().screen_plane();
        self.layer.write_with(
            &mut builder,
            &plane,
            self.features.layer_open,
            Some(&layers),
            |builder| {
                if visible && !manual {
                    game_hud::hud::orb_marks::write(
                        self.show.lit(),
                        table_now,
                        table,
                        &plane,
                        self.layer.icons(),
                        builder,
                    );
                }
            },
        );
        builder.finish();
        self.finish_frame(table_now, transitioning);
        self.place_story();
        &self.frame
    }

    /// Updates hover over the drawn frame and schedules its successor.
    fn finish_frame(&mut self, table_now: u64, transitioning: bool) {
        self.features.prompts.placed_let_go(&mut self.show);
        self.show.point(&mut self.frame, table_now);
        // The inspector and a pile's count follow what was just found under the pointer in the
        // next frame, and a prompt whose view has just played out draws its marks in it.
        let own_due = self.features.prompts.due() && self.show.settled();
        let inspecting = self.features.inspector().needs_frame(self.hovered_slot());
        self.wake = if transitioning
            || own_due
            || inspecting
            || self
                .features
                .piles()
                .due(self.show.pointer().hovered_pile())
        {
            Wake::Now
        } else {
            self.timed_wake(table_now)
        };
    }

    /// The spans and streaks the table shows.
    #[must_use]
    pub fn lit(&self) -> &game_play::light::Lit {
        self.show.lit()
    }

    /// The spans and streaks the table shows, for the marks and arrows a prompt begins and ends
    /// itself, timed by the session's `own_timings`.
    pub fn lit_mut(&mut self) -> &mut game_play::light::Lit {
        self.show.lit_mut()
    }

    /// Lays the table and the flat layer out in a window of a new size, scale or safe area.
    fn resize(&mut self, window: Window) {
        self.cancel_gesture();
        self.routes.scrolling.cancel();
        let previous_scale = self.window.scale;
        self.window = self.data().profiles.resolve_window(window);
        let window = self.layout_window();
        self.show.resize(window);
        if (previous_scale - window.scale).abs() > f32::EPSILON {
            self.restart_story();
        }
        self.layer.resize(window);
        self.place_data_error();
        self.refresh_frame_rates();
        self.show_gear();
        self.prepare_resized();
        if self.show.badges_mut().shaped() {
            let glyphs = self.layer.glyphs_mut();
            (self.show.badges_mut()).rescale(window.scale, glyphs);
        }
        // A new size builds a shown screen again, which places its shown objects too: a phone's
        // screens span the window, which the keyboard shortens. Before anything is shown, as at
        // the host's first surface, the first update or the agents build it.
        if self.layer.showing() || self.features.screen == Screen::Gallery {
            self.show_screen();
        } else {
            self.place_shown();
        }
    }

    /// Clicks the open prompt, naming the decision and the prompt's state the click was made
    /// on. Does nothing while no prompt is open.
    fn click(&mut self, click: Click) {
        if let Some(request) = self.features.prompts.click(click, &mut self.show) {
            self.commands.push_back(SessionCommand::Click(request));
        }
    }

    /// From now on, keeps a record of every script the client is given, for the interface
    /// checks (`Show::record`).
    pub fn record(&mut self) {
        self.show.record();
        self.features.prompts.record();
    }

    /// Every step the prompts have made themselves since `record` was called, in order.
    #[must_use]
    pub fn own_history(&self) -> &[prompts::OwnRecord] {
        self.features.prompts.history()
    }

    /// The record of every script given since `record` was called, in order.
    #[must_use]
    pub fn history(&self) -> &[ScriptRecord] {
        self.show.history()
    }

    /// Plants a fault in a script not given yet, to test the interface checks.
    pub fn plant(&mut self, plant: Plant) {
        self.show.plant(plant);
    }

    /// When the table came to rest after the last script to end, in nanoseconds since the client
    /// started, however the frames fell.
    #[must_use]
    pub fn rested(&self) -> u64 {
        self.features.bug().frame_time(self.show.rested())
    }

    /// When the first notice shown or to be shown ends, as of the last `frame`, if there is one
    /// (`Show::notice_end`).
    #[must_use]
    pub fn notice_end(&self) -> Option<u64> {
        self.show
            .notice_end()
            .map(|end| self.features.bug().frame_time(end))
    }

    /// The frame the last `frame` made, without making another: what the host read back, for
    /// the element tree a bug report keeps beside its picture.
    #[must_use]
    pub fn last_frame(&self) -> &Frame {
        &self.frame
    }

    /// The pointer on the table, for tests that read the held card's lean.
    #[must_use]
    pub fn pointer(&self) -> &game_play::pointer::Pointer {
        self.show.pointer()
    }

    /// The flat layer, for tests that show an element of their own in place of the screen. The
    /// next change of screen, sheet or settings shows the screen again.
    pub fn layer_mut(&mut self) -> &mut Layer {
        &mut self.layer
    }

    /// The table, for tests and tools that read the camera.
    #[must_use]
    pub fn table(&self) -> &Table {
        self.show.table()
    }

    /// When the next frame is needed: as of the last call to `frame`, or `Now` once an input
    /// has arrived since, because any input ends the wait.
    #[must_use]
    pub fn wake(&self) -> Wake {
        if self.story_moving() {
            Wake::Now
        } else {
            self.wake.or_at(self.features.next_frame())
        }
    }

    /// When to paint a live frame, including decorative motion that never delays play.
    #[must_use]
    pub fn display_wake(&self) -> Wake {
        if self.features.opening.coin_moving || self.frame.ambient_motion {
            Wake::Now
        } else {
            self.wake()
        }
    }

    /// The next verb that a script played as an instant change because it is not built yet,
    /// each verb returned once, for the host to log.
    pub fn next_unbuilt(&mut self) -> Option<Verb> {
        self.show.next_unbuilt()
    }

    /// How many script steps the table played over could not take so far; each made the step
    /// after it place the whole table instead of moving its cards.
    #[must_use]
    pub fn unplaced(&self) -> usize {
        self.show.unplaced()
    }

    /// The verbs of the script playing as of the last `frame`, for the host's log of late
    /// frames.
    pub fn playing_verbs(&self) -> impl Iterator<Item = Verb> + '_ {
        let verbs = self.show.playing_verbs();
        Verb::ALL
            .iter()
            .enumerate()
            .filter(move |(index, _)| verbs & (1 << index) != 0)
            .map(|(_, verb)| *verb)
    }

    /// The sounds that begin in the last frame made (`Frame::sounds`), for the host to play.
    #[must_use]
    pub fn sounds(&self) -> &[Sound] {
        &self.frame.sounds
    }

    /// The next thing the client asks of the platform, if any.
    pub fn next_request(&mut self) -> Option<Request> {
        self.requests.pop_front()
    }

    /// The next command for the session, if any. The host sends it to the session thread and
    /// passes the answer back as `Input::Reply`.
    pub fn next_command(&mut self) -> Option<SessionCommand> {
        self.commands.pop_front()
    }

    /// Physical pixels per logical point, used to prepare icons and prewarm text for the surface.
    #[must_use]
    pub fn surface_scale(&self) -> f32 {
        self.window.scale
    }

    /// The glyphs that laid-out text needs and no atlas page holds yet. Each is returned once.
    /// The host hands them to the rasterizer.
    pub fn wanted_glyphs(&mut self) -> impl Iterator<Item = GlyphKey> + '_ {
        let story = self.stories.wanted_glyphs();
        self.layer.glyphs_mut().drain_wanted().chain(story)
    }

    /// The faces a card being read needs at full size and has not been asked for yet (a card in
    /// the inspector). Each is returned once. The host hands them to its asset workers, which
    /// report each one's full-size layer as an `AssetReady::Face`.
    pub fn wanted_faces(&mut self) -> impl Iterator<Item = FaceId> + '_ {
        let story = self.active_story_faces();
        let preview = story.then(|| {
            self.stories
                .board_mut()
                .expect("active story board")
                .wanted_faces()
        });
        let main = (!story).then(|| self.show.faces_mut().drain_wanted());
        main.into_iter()
            .flatten()
            .chain(preview.into_iter().flatten())
    }

    /// The faces the table shows at full size now (`Faces::shown_full`), whose full-size layers
    /// the host's asset workers reuse last. None while a story's board is shown.
    #[must_use]
    pub fn shown_full_faces(&self) -> &[FaceId] {
        if self.active_story_faces() {
            &[]
        } else {
            self.show.faces().shown_full()
        }
    }
}

/// Updates the HUD features (totals, piles, notices, rows and banner) for the current frame.
fn update_hud_features(
    features: &mut features::Features,
    layer: &mut Layer,
    show: &Show,
    (table_now, tier): (u64, Tier),
    (visible, counted, rows_counted, covers): (bool, bool, bool, bool),
    targets: [bool; 2],
) {
    let table = show.table();
    // Each total shows what `prepare` laid out, over the opening too, as the web's does.
    let opening = features.opening.shown;
    features.totals(counts::Draw {
        layer,
        counts: show.counts(),
        effects: show.effects(),
        now: table_now,
        table,
        tier,
        visible: counted,
        targets,
        dimmed: opening,
    });
    features.update_piles(piles::Draw {
        layer,
        table,
        hovered: show.pointer().hovered_pile(),
        visible: counted && !covers,
    });
    features.notices(notices::Draw {
        layer,
        notices: show.notices(),
        now: table_now,
        layout: table.layout(),
        visible,
    });
    features.update_rows(rows::Draw {
        layer,
        table,
        visible: rows_counted,
    });
    features.banner(banner::Draw {
        layer,
        view: show.shown().map(|update| &update.view),
        now: table_now,
        visible,
        tuning: table.data().hud,
    });
}
