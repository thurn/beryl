//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "Components" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

mod bug;
mod carry;
mod editing;
mod entries;
mod event;
pub(crate) use crate::command::{Reply, Request, SessionCommand};
pub use event::Input;
mod gesture;

use std::collections::VecDeque;

use beryl_protocol::prompt::{Click, ClickRequest, PromptView};
use beryl_protocol::protocol::{DecisionId, SessionId, Side, Status};
use beryl_protocol::script::{OwnTimings, Verb, own_timings};
use beryl_protocol::{AgentView, StartRequest, StopsSetting};

use crate::screens::{self, Concede, GalleryState, Music, SEED, Screen, Sheet, StartForm};
use crate::settings::Settings;
use game_hud::hud::banner::Banner;
use game_hud::hud::log::Log;
use game_hud::hud::piles::PileCounts;
use game_hud::hud::rows::RowCounts;
use game_hud::prompts;
use game_play::hud::counts::CountFloats;
use game_play::hud::notices::NoticeFloats;
use game_play::play::Show;
use game_play::play::record::{Plant, ScriptRecord};
use game_play::pointer::LetGo;
use game_play::pointer::rows::RowGesture;
use game_play::pointer::touch::Touch;
use game_table::faces::{CARD_BACK, FaceList, Faces};
use game_table::table::{Table, Window};
use kit_frame::frame::{self, ElementId, Frame, FrameBuilder, Sound, Wake};
use kit_ui::input::{FaceId, FaceSize, GlyphKey, Lifecycle, Tier};
use kit_ui::layout::{self, Layer};
use kit_ui::widgets::Fields;

mod input;
mod inspect;
mod lobby;
mod manual;
mod manual_overlays;
mod own;
mod press;
pub use crate::screens::BugPress;
pub use press::{Dismiss, Press};
mod refusal;
mod screen;
mod stops;
mod table_pointer;
mod testing;
mod touch;
mod viewing;

use inspect::Inspector;
use viewing::Viewing;

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
    /// Inputs `handle` has stored and the next `frame` has not taken yet: those no part of the
    /// client acts on yet.
    inputs: Vec<Input>,
    window: Window,
    /// The table and the scripts that lead it from view to view.
    show: Show,
    /// The flat layer: the screen, the sheet over it and the floats.
    layer: Layer,
    /// The floats that show the players' life totals and pools.
    counts: CountFloats,
    /// The floats that show how many cards each pile holds.
    piles: PileCounts,
    /// The floats that show the notices `announce` steps make.
    notices: NoticeFloats,
    /// The counts of the cards scrolled off each end of a battlefield row.
    row_counts: RowCounts,
    /// The step banner under the opponent's orb.
    banner: Banner,
    /// The game log, as the views have told it so far, and whether it is folded and open.
    log: Log,
    /// What the settings sheet's concede button does when pressed (`ScreenIn::concede`).
    concede: Concede,
    /// What the pointer is doing to scroll a battlefield row.
    row_gesture: RowGesture,
    /// The screen the flat layer shows. A recording or a game is fed to the client on the game
    /// screen, which it starts on.
    screen: Screen,
    gallery: GalleryState,
    /// The static samples have their own face list, loaded only when the gallery opens.
    gallery_faces: Option<Faces>,
    /// The sheet open over the screen, if any.
    sheet: Option<Sheet>,
    settings: Settings,
    /// The agents a person may play, as the host listed them.
    agents: Vec<AgentView>,
    /// What the person has chosen on the start screen.
    start: StartForm,
    /// The screen the first view of all opens, if the client is on the start screen only
    /// because the agents arrived before any view: the game, which the host resumed or started.
    first_view_opens: Option<Screen>,
    /// The names of the recordings the testing screen lists, as the host listed them.
    recordings: Vec<String>,
    /// The seat the testing screen replays recordings from.
    seat: Side,
    /// The session the latest view came from, which stop and concede commands name.
    session: Option<SessionId>,
    /// The stops the game holds, as far as the client knows: the latest view's, or those it
    /// sent since (`send_stops`).
    stops_held: Option<StopsSetting>,
    /// The music last asked of the host, so that it is asked for again only when it changes.
    music: Option<Music>,
    /// The timing of every step the client makes itself, read once at startup.
    own_timings: OwnTimings,
    /// The open decision and its prompt's latest state, from the view or a click's reply.
    prompt: Option<(DecisionId, PromptView)>,
    /// The prompt's state before its latest change, and whether the next `frame` still has to
    /// call `prompts::own` for that change at its `now`. Kept here, not cloned in `frame`, so
    /// that a frame allocates nothing.
    was: Option<(DecisionId, PromptView)>,
    own_due: bool,
    suggesting: Suggesting,
    /// What the prompt's own steps depend on besides the prompt (`own::Owned`).
    owned: own::Owned,
    /// How many views the client has been given, and, once `record` is called, every step the
    /// prompts have made themselves since.
    views: usize,
    own_record: Option<Vec<prompts::OwnRecord>>,
    /// Whether the prompt bar is drawn and takes presses. The bar of the latest view is laid out
    /// as the view arrives, because layout runs in `handle`, but it opens only once that view's
    /// script and every script before it have played out (`Show::settled`), as the web
    /// interface's prompt opens when its view has played out. Set by each `frame`, so a press
    /// reaches only a bar the last frame drew. The bar waits for it (`Element::waits`).
    open: bool,
    /// What the primary pointer is doing since it went down (`gesture`), and the screen and
    /// sheet the flat layer last showed, a change of which ends it.
    gesture: gesture::Gesture,
    shown: (Screen, Option<Sheet>),
    /// The host's platform, which says what `Cmd` in a shortcut means.
    platform: Platform,
    /// What each text field holds, which the screens show (`editing`).
    fields: Fields,
    /// The text field the host was last asked to begin text entry over, if any.
    entry: Option<ElementId>,
    /// When and where the primary button last went down in the edited field, which makes the
    /// next press there a double click.
    last_down: Option<(u64, glam::Vec2)>,
    /// The `now` of the latest frame: the time of every input since, which has none of its own.
    now: u64,
    wake: Wake,
    requests: VecDeque<Request>,
    commands: VecDeque<SessionCommand>,
    /// The decision of each click sent and not yet answered, oldest first. The session answers
    /// commands in order, so each `Reply::Click` is for the oldest.
    clicks: VecDeque<DecisionId>,
    tier: Tier,
    /// Whether the host has said memory is low (`Lifecycle::LowMemory`), which fixes the tier at
    /// Baseline.
    low_memory: bool,
    /// What the inspector shows and when to lay it out again (`inspect`).
    inspector: Inspector,
    /// The viewer of a pile's cards and the Revealed Cards pill (`viewing`).
    viewing: Viewing,
    /// The bug report and the table's clock, which it freezes (`bug`).
    bug: bug::Bug,
    /// What the manual table shows over itself (`manual`).
    manual: manual::Manual,
    /// The fingers on a touch screen (`touch`).
    touch: Touch,
}

/// Whether the pointer is on the open prompt's button that shows what it would tap
/// (`prompts::shows`), and whether it was when `prompts::own` last ran, beside `Client::was`.
#[derive(Clone, Copy, Debug, Default)]
struct Suggesting {
    now: bool,
    was: bool,
}

impl Client {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            frame: Frame::with_capacity(config.elements, config.draws, config.name_bytes),
            inputs: Vec::new(),
            window: Window::default(),
            show: Show::new(Table::default()),
            layer: Layer::new(),
            counts: CountFloats::default(),
            piles: PileCounts::default(),
            notices: NoticeFloats::default(),
            row_counts: RowCounts::default(),
            banner: Banner::default(),
            log: Log::default(),
            concede: Concede::Asks,
            row_gesture: RowGesture::default(),
            screen: Screen::Game,
            gallery: GalleryState::default(),
            gallery_faces: None,
            sheet: None,
            settings: Settings::default(),
            agents: Vec::new(),
            start: StartForm::default(),
            first_view_opens: None,
            recordings: Vec::new(),
            seat: Side::You,
            session: None,
            stops_held: None,
            music: None,
            own_timings: own_timings(),
            prompt: None,
            was: None,
            own_due: false,
            suggesting: Suggesting::default(),
            owned: own::Owned::default(),
            views: 0,
            own_record: None,
            open: false,
            gesture: gesture::Gesture::None,
            shown: (Screen::Game, None),
            platform: Platform::Mac,
            fields: Fields::new(),
            entry: None,
            last_down: None,
            now: 0,
            wake: Wake::Never,
            requests: VecDeque::new(),
            commands: VecDeque::new(),
            clicks: VecDeque::new(),
            tier: Tier::Baseline,
            low_memory: false,
            inspector: Inspector::default(),
            viewing: Viewing::default(),
            bug: bug::Bug::default(),
            manual: manual::Manual::default(),
            touch: Touch::default(),
        }
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
            Input::Update(update) => {
                // The first view of all is of a game the host resumed or started, which may
                // reach the client after the agents put it on the start screen.
                if let Some(screen) = self.first_view_opens.take() {
                    self.screen = screen;
                }
                self.views += 1;
                self.take_session(&update);
                self.set_source(&update.view.status);
                self.set_prompt(match &update.view.status {
                    Status::Deciding { decision } => {
                        Some((decision.id, (*decision.prompt).clone()))
                    }
                    _ => None,
                });
                self.prepare_pill(&update.view);
                self.stops_held = Some(update.view.stops);
                self.send_stops();
                // The same view sent again says nothing new happened.
                let again = self
                    .show
                    .latest()
                    .is_some_and(|latest| latest.view == update.view);
                if !again {
                    self.log.logged.take(&update.view);
                }
                Banner::prepare(&mut self.layer, &update.view);
                self.show.push_update(update);
                // Every life total and pool the scripts to come will show is laid out now, so
                // that a frame lays out none of them.
                self.prepare_counts();
                // And so is every notice they will show.
                NoticeFloats::prepare(
                    &mut self.layer,
                    self.show.coming_notices(),
                    self.show.notices(),
                );
                let (text, glyphs) = self.layer.text_and_glyphs();
                self.show.shape_badges(text, glyphs, self.window.scale);
                let shown = self.show.shown().map(|update| &update.view);
                let coming = self.show.coming_counts().map(|(_, view)| view);
                let table = self.show.table();
                (self.piles).prepare(&mut self.layer, table, shown.into_iter().chain(coming));
                (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
                self.show_screen();
            }
            Input::Reply(Reply::CardSearch(text, hits)) => self.take_token_hits(text, hits),
            Input::Reply(Reply::TableAct(reply)) => self.take_table_reply(reply),
            Input::Reply(Reply::Click(Ok(reply))) => self.click_replied(reply),
            Input::Reply(Reply::Click(Err(_))) => {
                self.clicks.pop_front();
                self.inputs.push(input);
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
                self.settings = settings;
                self.send_stops();
                self.show_screen();
            }
            Input::Agents(agents) => self.take_agents(agents),
            Input::Recordings(recordings) => {
                self.recordings = recordings;
                self.show_screen();
            }
            Input::BugSaved(result) => self.bug_saved(result),
            Input::Nudge => {}
            Input::Pointer(pointer) => self.point(pointer),
            Input::Key(key) => self.key(key),
            Input::Platform { mac } => {
                self.platform = if mac { Platform::Mac } else { Platform::Other };
            }
            Input::Cards(records) => {
                let list = FaceList::from_records(&records);
                self.show.faces_mut().replace(list);
            }
            Input::Assets(asset) => self.asset_ready(asset),
            Input::Surface {
                size,
                scale,
                safe_area,
            } => {
                // A finger's gesture ends with the layout it began on.
                self.cancel_touch();
                self.resize(Window {
                    size,
                    scale,
                    safe_area,
                });
            }
            Input::Text(_) | Input::Paste(_) => self.type_into(&input),
            Input::Tier(tier) => self.set_tier(tier),
            Input::Lifecycle(Lifecycle::LowMemory) => self.low_memory(),
            input => self.inputs.push(input),
        }
        self.follow_entry();
        self.wake = Wake::Now;
    }

    /// Takes the tier the host's rate rule chose, unless memory has run low, after which the
    /// tier stays Baseline (docs/beryl-3d.md, "Choosing a tier").
    fn set_tier(&mut self, tier: Tier) {
        if !self.low_memory {
            self.tier = tier;
        }
    }

    /// Memory is low (docs/beryl-3d.md, "Hosts": "Low memory"): forgets every full-size face,
    /// which the renderer has freed, and draws in the Baseline tier for the rest of the session.
    fn low_memory(&mut self) {
        self.low_memory = true;
        self.tier = Tier::Baseline;
        self.show.faces_mut().low_memory();
    }

    /// Whether the hud's floats (the life totals, pools, pile counts and notices) show: on the
    /// game screen and the manual table, with no sheet over them but a bug report capturing the
    /// table (`table_shown`).
    fn hud_shown(&self) -> bool {
        matches!(self.screen, Screen::Game | Screen::Table) && self.table_shown()
    }

    /// Whether each frame records how its flat-layer elements look and the words of each line
    /// of text (`Frame::looking`), for the tools that compare Beryl 3D with the web interface.
    pub fn set_looking(&mut self, looking: bool) {
        self.frame.looking = looking;
    }

    /// The frame at `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame {
        self.now = now;
        // The table plays on its own clock, which the bug report stops (`bug`).
        let table_now = self.bug_frame(now);
        self.prepare_table(table_now);
        let reporting = self.bug_open();
        self.inputs.clear();
        self.touch_frame(now);
        self.own(table_now);
        self.set_aims(self.table_open());
        self.table_frame(table_now);
        let covers = self.prompt_covers_table();
        self.show.cover_table(covers);
        let targets = self.targets(self.table_open());
        let transitioning = self.tick_layer(now) | self.shake_button(table_now);
        self.show.pointer_mut().advance(table_now);
        self.show.advance_slide(table_now);
        self.inspect(table_now);
        self.viewing_frame(table_now);
        let visible = self.hud_shown();
        // The guided start's opening covers the table's life totals and pile counts, not notices.
        let counted = visible && !self.guided_start();
        let rows_counted = self.rows_counted(visible, covers);
        let mut builder = FrameBuilder::new(&mut self.frame, self.tier);
        // The flat layer is placed for the table's camera, so the frame takes it even when no
        // game view writes the table, as on the start screen (the "layer drawn" check).
        builder.camera(self.show.table().camera().view_projection);
        builder.sounds(self.manual.take_refusal());
        self.show.frame(table_now, &mut builder);
        // Elements anchored to a card, an ability or a player stand where the table drew it.
        let table = self.show.table();
        self.layer
            .follow(|subject| game_table::table::spot_of(&builder, table, subject));
        // Each total shows what `prepare` laid out, over the opening too, as the web's does.
        self.counts.write(
            &mut self.layer,
            self.show.counts(),
            self.show.effects(),
            table_now,
            self.show.table().layout(),
            self.window.size,
            (counted, targets, self.viewing.opening),
        );
        (self.piles).write(&mut self.layer, &self.show, counted && !covers);
        (self.notices).write(
            &mut self.layer,
            self.show.notices(),
            table_now,
            (self.show.table().layout(), self.window.size),
            visible,
        );
        (self.row_counts).write(&mut self.layer, self.show.table(), rows_counted);
        let at_rest = self.show.shown().map(|update| &update.view);
        (self.banner).write(&mut self.layer, at_rest, table_now, self.window, visible);
        let (layout_nodes, text_runs) = self.layer.take_work();
        builder.layout_work(layout_nodes, text_runs);
        builder.placement_work(self.show.table().take_placed(), self.show.take_compiled());
        // The bug report is drawn whether or not the table has settled (`bug`).
        self.open = reporting || self.show.settled();
        let faces = if self.screen == Screen::Gallery {
            self.gallery_faces.as_ref()
        } else {
            Some(self.show.faces())
        };
        // While scripts play only what waits for the table (`Element::waits`) is left out.
        let face_layers = |face, size| match size {
            FaceSize::Board => faces?.board_layer_of(face),
            FaceSize::Full => faces?.full_layer_of(face),
        };
        let layers = faces.map(|_| layout::FaceLayers {
            back: CARD_BACK,
            layer: &face_layers,
        });
        self.layer.write_with(
            &mut builder,
            &self.show.table().camera().screen_plane(),
            self.open,
            layers.as_ref(),
        );
        builder.finish();
        self.placed_let_go();
        // The element a press under the mouse would reach is drawn `hovered`. A press on a card
        // that ended since the last frame and means a click on it (a press that picked nothing
        // up, or a card carried off the hand and let go) clicks the open prompt on that card;
        // one held up in a tray and carried is dropped where it was let go.
        match self.show.point(&mut self.frame, table_now) {
            Some(LetGo::Click(slot)) => self.card_clicked(slot),
            Some(LetGo::Tap(slot)) => self.card_tapped(slot),
            Some(LetGo::Dropped { slot, at }) if self.screen == Screen::Table => {
                self.table_drop(slot, at);
            }
            Some(LetGo::Dropped { slot, at }) => self.drop_held(slot, at),
            None => {}
        }
        // The inspector and a pile's count follow what was just found under the pointer in the
        // next frame, and a prompt whose view has just played out draws its marks in it.
        let own_due = self.own_due && self.show.settled();
        let inspecting = self.inspector.due || self.hovered_slot() != self.inspector.judged;
        self.wake = if transitioning || own_due || inspecting || self.piles.due(&self.show) {
            Wake::Now
        } else {
            self.timed_wake(table_now)
        };
        &self.frame
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
        self.end_gesture();
        self.window = window;
        self.show.resize(window);
        self.layer.resize(window);
        self.show_gear();
        self.prepare_resized();
        if self.show.badges_mut().shaped() {
            let glyphs = self.layer.glyphs_mut();
            (self.show.badges_mut()).rescale(window.scale, glyphs);
        }
        // A new size builds a shown screen again, which places its shown objects too: a phone's
        // screens span the window, which the keyboard shortens. Before anything is shown, as at
        // the host's first surface, the first update or the agents build it.
        if self.layer.showing() || self.screen == Screen::Gallery {
            self.show_screen();
        } else {
            self.place_shown();
        }
    }

    /// Starts a game with what the start screen holds, as the web's `StartScreen` does: nothing
    /// while no opponent is chosen or the seed is not one; with a game in progress, the first
    /// press only asks to confirm ("Concede & Start") and the next concedes that game and then
    /// starts the new one, whether or not the concession lands first.
    fn start_game(&mut self) {
        let Some(agent) = (self.start.agent).and_then(|at| self.agents.get(usize::from(at))) else {
            return;
        };
        let Ok(seed) = screens::seed(&self.fields.get(SEED).text) else {
            return;
        };
        let view = self.show.latest().map(|update| &update.view);
        let going_on = self.session.filter(|_| screens::in_progress(view));
        if going_on.is_some() && !self.start.confirming {
            self.start.confirming = true;
            self.show_screen();
            return;
        }
        if let Some(session) = going_on {
            self.commands.push_back(SessionCommand::Concede(session));
        }
        self.commands.push_back(SessionCommand::Start(StartRequest {
            agent: agent.name.clone(),
            seed,
            fair: self.start.fair,
        }));
        self.start.confirming = false;
        self.first_view_opens = None;
        self.screen = Screen::Game;
        self.set_sheet(None);
        self.show_screen();
    }

    /// Clicks the open prompt, naming the decision and the prompt's state the click was made
    /// on. Does nothing while no prompt is open.
    fn click(&mut self, click: Click) {
        let Some((decision, prompt)) = &self.prompt else {
            return;
        };
        self.clicks.push_back(*decision);
        self.commands.push_back(SessionCommand::Click(ClickRequest {
            decision: *decision,
            revision: prompt.revision,
            click,
        }));
    }

    /// From now on, keeps a record of every script the client is given, for the interface
    /// checks (`Show::record`).
    pub fn record(&mut self) {
        self.show.record();
        self.own_record.get_or_insert_default();
    }

    /// Every step the prompts have made themselves since `record` was called, in order.
    #[must_use]
    pub fn own_history(&self) -> &[prompts::OwnRecord] {
        self.own_record.as_deref().unwrap_or_default()
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
        self.bug.frame_time(self.show.rested())
    }

    /// When the first notice shown or to be shown ends, as of the last `frame`, if there is one
    /// (`Show::notice_end`).
    #[must_use]
    pub fn notice_end(&self) -> Option<u64> {
        self.show.notice_end().map(|end| self.bug.frame_time(end))
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
        self.wake
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

    /// The glyphs that laid-out text needs and no atlas page holds yet. Each is returned once.
    /// The host hands them to the rasterizer.
    pub fn wanted_glyphs(&mut self) -> impl Iterator<Item = GlyphKey> + '_ {
        self.layer.glyphs_mut().drain_wanted()
    }

    /// The faces a card being read needs at full size and has not been asked for yet (a card in
    /// the inspector). Each is returned once. The host hands them to its asset workers, which
    /// report each one's full-size layer as an `AssetReady::Face`.
    pub fn wanted_faces(&mut self) -> impl Iterator<Item = FaceId> + '_ {
        self.show.faces_mut().drain_wanted()
    }
}
