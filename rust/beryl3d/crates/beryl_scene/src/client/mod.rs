//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "Components" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

mod editing;

use std::collections::VecDeque;

use beryl_protocol::prompt::{Click, ClickReply, ClickRequest, PromptView};
use beryl_protocol::protocol::{DecisionId, ObjectId, SessionId, Side, Status, StepSet, ZoneName};
use beryl_protocol::script::{OwnTimings, Subject, Verb, own_timings};
use beryl_protocol::{AgentView, StartRequest};

use crate::command::{Reply, Request, SessionCommand};
use crate::element::Press;
use crate::faces::{FaceList, Faces};
use crate::frame::{self, ElementId, Frame, FrameBuilder, Place, Sound, Wake};
use crate::hud::counts::CountFloats;
use crate::hud::notices::NoticeFloats;
use crate::hud::piles::PileCounts;
use crate::hud::rows::RowCounts;
use crate::input::{AssetReady, FaceId, GlyphKey, Input, Lifecycle, Tier};
use crate::layout::{self, Layer};
use crate::play::Show;
use crate::play::record::{Plant, ScriptRecord};
use crate::pointer::rows::RowGesture;
use crate::prompts;
use crate::screens::{self, Music, SEED, Screen, ScreenIn, Sheet, StartForm};
use crate::settings::{Setting, Settings};
use crate::table::{Table, Window};
use crate::widgets::Fields;

mod input;
mod inspect;
mod own;
mod testing;

use inspect::Inspector;

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
    /// What the pointer is doing to scroll a battlefield row.
    row_gesture: RowGesture,
    /// The screen the flat layer shows. A recording or a game is fed to the client on the game
    /// screen, which it starts on.
    screen: Screen,
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
    /// reaches only a bar the last frame drew.
    open: bool,
    /// The element the primary pointer went down on, which a release on it presses.
    pressed: Option<ElementId>,
    /// The finger dragging a scrolling element of the flat layer, that element, and where the
    /// finger was last.
    scroll_drag: Option<(u8, ElementId, glam::Vec2)>,
    /// The slider the primary pointer went down on, which a drag sets until the press ends.
    slider: Option<ElementId>,
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
    tier: Tier,
    /// Whether the host has said memory is low (`Lifecycle::LowMemory`), which fixes the tier at
    /// Baseline.
    low_memory: bool,
    /// What the inspector shows and when to lay it out again (`inspect`).
    inspector: Inspector,
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
            row_gesture: RowGesture::default(),
            screen: Screen::Game,
            sheet: None,
            settings: Settings::default(),
            agents: Vec::new(),
            start: StartForm::default(),
            first_view_opens: None,
            recordings: Vec::new(),
            seat: Side::You,
            session: None,
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
            pressed: None,
            scroll_drag: None,
            slider: None,
            platform: Platform::Mac,
            fields: Fields::new(),
            entry: None,
            last_down: None,
            now: 0,
            wake: Wake::Never,
            requests: VecDeque::new(),
            commands: VecDeque::new(),
            tier: Tier::Baseline,
            low_memory: false,
            inspector: Inspector::default(),
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
                self.session = Some(update.session);
                self.set_source(&update.view.status);
                self.set_prompt(match &update.view.status {
                    Status::Deciding { decision } => {
                        Some((decision.id, (*decision.prompt).clone()))
                    }
                    _ => None,
                });
                // The session sends one timing for each step of the script beside it.
                let timings = update
                    .view
                    .script
                    .as_ref()
                    .map(|script| {
                        debug_assert_eq!(
                            script.timing.len(),
                            script.steps(),
                            "the session sends one timing per step"
                        );
                        script.timing.clone()
                    })
                    .unwrap_or_default();
                self.show.push(update, timings);
                // Every life total and pool the scripts to come will show is laid out now, so
                // that a frame lays out none of them.
                self.prepare_counts();
                // And so is every notice they will show.
                NoticeFloats::prepare(
                    &mut self.layer,
                    self.show.coming_notices(),
                    self.show.notices(),
                );
                if !self.show.badges_mut().shaped() {
                    let (text, glyphs) = self.layer.text_and_glyphs();
                    (self.show.badges_mut()).shape(text, glyphs, self.window.scale);
                }
                let shown = self.show.shown().map(|update| &update.view);
                let coming = self.show.coming_counts().map(|(_, view)| view);
                (self.piles).prepare(&mut self.layer, shown.into_iter().chain(coming));
                (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
                self.show_screen();
            }
            Input::Reply(Reply::Click(Ok(reply))) => {
                match reply {
                    ClickReply::Open { prompt } => {
                        let decision = self.prompt.as_ref().map(|(decision, _)| *decision);
                        self.set_prompt(decision.map(|decision| (decision, *prompt)));
                    }
                    // The views that follow the answer bring the next prompt.
                    ClickReply::Answered => self.set_prompt(None),
                }
                self.prepare_counts();
                self.show_screen();
            }
            Input::Settings(settings) => {
                self.settings = settings;
                self.show_screen();
            }
            Input::Agents(agents) => self.take_agents(agents),
            Input::Recordings(recordings) => {
                self.recordings = recordings;
                self.show_screen();
            }
            Input::Nudge => {}
            Input::Pointer(pointer) => self.point(pointer),
            Input::Key(key) => self.key(key),
            Input::Platform { mac } => {
                self.platform = if mac { Platform::Mac } else { Platform::Other };
            }
            Input::Cards(records) => {
                *self.show.faces_mut() = Faces::new(FaceList::from_records(&records));
            }
            Input::Assets(AssetReady::Face { face, size, layer }) => {
                self.show.faces_mut().arrived(face, size, layer);
            }
            Input::Assets(AssetReady::Glyphs { page, glyphs }) => {
                self.layer.glyphs_mut().add(page, &glyphs);
                self.show.badges_mut().add(page, &glyphs);
            }
            Input::Assets(AssetReady::Icons { page, cells }) => {
                self.layer.icons_mut().add(page, &cells);
            }
            Input::Surface {
                size,
                scale,
                safe_area,
            } => self.resize(Window {
                size,
                scale,
                safe_area,
            }),
            Input::Text(_) | Input::Paste(_) => self.type_into(&input),
            Input::Tier(tier) => self.set_tier(tier),
            Input::Lifecycle(Lifecycle::LowMemory) => self.low_memory(),
            input => self.inputs.push(input),
        }
        self.follow_entry();
        self.wake = Wake::Now;
    }

    /// Keeps the agents a person may play and chooses the first to offer. The live app sends
    /// them once it has resumed a game or found none to resume, so agents that arrive before
    /// any view mean there is no game yet: the person starts one on the start screen.
    fn take_agents(&mut self, agents: Vec<AgentView>) {
        if self.views == 0 {
            self.screen = Screen::Start;
            self.first_view_opens = Some(Screen::Game);
        }
        self.start.agent = StartForm::first_agent(&agents);
        self.agents = agents;
        self.show_screen();
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

    /// The frame at `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame {
        self.now = now;
        self.inputs.clear();
        self.own(now);
        self.set_aims(self.open);
        let targets = self.targets(self.open);
        let transitioning = self.layer.tick(now);
        self.show.pointer_mut().advance(now);
        self.show.advance_slide(now);
        self.inspect();
        let mut builder = FrameBuilder::new(&mut self.frame);
        self.show.frame(now, &mut builder);
        // Elements anchored to a card, an ability or a player stand where the table drew it.
        let table = self.show.table();
        self.layer
            .follow(|subject| layout::centre_of(&builder, table, subject));
        // A total that changes shows the element `prepare` laid out for it.
        self.counts.write(
            &mut self.layer,
            self.show.counts(),
            self.show.effects(),
            now,
            self.show.table().layout(),
            self.window.size,
            (self.screen == Screen::Game && self.sheet.is_none(), targets),
        );
        let visible = self.screen == Screen::Game && self.sheet.is_none();
        (self.piles).write(&mut self.layer, self.show.table(), visible);
        (self.notices).write(
            &mut self.layer,
            self.show.notices(),
            now,
            (self.show.table().layout(), self.window.size),
            visible,
        );
        (self.row_counts).write(&mut self.layer, self.show.table(), visible);
        let (layout_nodes, text_runs) = self.layer.take_work();
        builder.layout_work(layout_nodes, text_runs);
        builder.placement_work(self.show.table().take_placed(), self.show.take_compiled());
        self.open = self.show.settled();
        let faces = Some(self.show.faces());
        (self.layer).write_with(&mut builder, self.show.table().camera(), self.open, faces);
        builder.finish();
        // The element a press under the mouse would reach is drawn `hovered`. A press on a card
        // that ended since the last frame and means a click on it (a press that picked nothing
        // up, or a card carried off the hand and let go) clicks the open prompt on that card.
        if let Some(slot) = self.show.point(&mut self.frame, now)
            && let Ok(slot) = u16::try_from(slot)
        {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(Click::On { on });
        }
        // The inspector follows the card just found under the pointer in the next frame, and a
        // prompt whose view has just played out draws its marks in the next frame.
        let own_due = self.own_due && self.show.settled();
        self.wake = if transitioning || own_due || self.hovered_slot() != self.inspector.judged {
            Wake::Now
        } else {
            // A tooltip waiting for the pointer to rest shows at its time.
            self.show.wake(now).or_at(self.layer.tip_due())
        };
        &self.frame
    }

    /// The spans and streaks the table shows.
    #[must_use]
    pub fn lit(&self) -> &crate::light::Lit {
        self.show.lit()
    }

    /// The spans and streaks the table shows, for the marks and arrows a prompt begins and ends
    /// itself, timed by the session's `own_timings`.
    pub fn lit_mut(&mut self) -> &mut crate::light::Lit {
        self.show.lit_mut()
    }

    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    fn show_screen(&mut self) {
        self.inspector.due = true;
        let view = self.show.latest().map(|update| &update.view);
        let input = ScreenIn {
            window: self.window,
            view,
            prompt: self.prompt.as_ref().map(|(_, prompt)| prompt),
            settings: &self.settings,
            hovered: None,
            sheet: self.sheet,
            agents: &self.agents,
            start: &self.start,
            recordings: &self.recordings,
            seat: self.seat,
            fields: &self.fields,
            inspected: None,
            faces: Some(self.show.faces().list()),
        };
        // Until the sheets are built, an open sheet takes the screen's place in the flat layer.
        // The game screen shows nothing while no prompt is open and the game goes on, so that no
        // empty panel is drawn over the table (and none is built); once the game has ended it
        // shows the result.
        let ended =
            view.is_some_and(|view| matches!(view.status, Status::Over { .. } | Status::Stopped));
        let shown = match self.sheet {
            Some(sheet) => Some(screens::sheet(sheet, &input)),
            None if self.screen == Screen::Game && input.prompt.is_none() && !ended => None,
            None => Some(screens::screen(self.screen, &input)),
        };
        let music = screens::music(self.screen, view);
        self.layer.show(shown);
        if music != self.music {
            self.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// Lays the table and the flat layer out in a window of a new size, scale or safe area.
    fn resize(&mut self, window: Window) {
        self.window = window;
        self.show.resize(window);
        self.layer.resize(window);
        (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
        if self.show.badges_mut().shaped() {
            let glyphs = self.layer.glyphs_mut();
            (self.show.badges_mut()).rescale(window.scale, glyphs);
        }
    }

    /// Whether a press may pick up the card in `slot`, resting at `place`: a card in the
    /// viewer's hand that the open prompt marks, once the prompt is drawn. This is the web
    /// interface's `#lifts` in `ui/src/stage/prompt.ts`, except that a card in a pile of a tray
    /// is not yet holdable: the prompts of a split or an order add those piles.
    fn holdable(&self, slot: usize, place: Place) -> bool {
        let Some((_, prompt)) = self.prompt.as_ref().filter(|_| self.open) else {
            return false;
        };
        let in_hand = matches!(
            place,
            Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            }
        );
        let on = u16::try_from(slot).map(|slot| Subject::Object {
            slot: ObjectId(slot),
        });
        in_hand && (prompt.marks.iter()).any(|marked| Ok(marked.on) == on)
    }

    /// Carries out what a press on an element means: a command for the session, a request of
    /// the host, or a change of screen, sheet or settings. Every `Press` is interpreted here
    /// and nowhere else.
    pub fn press(&mut self, press: &Press) {
        match *press {
            Press::Button(button) => self.click(Click::Button { button }),
            Press::Prompt(click) => self.click(click),
            Press::Open(screen) => {
                self.first_view_opens = None;
                self.screen = screen;
                self.sheet = None;
                self.show_screen();
            }
            Press::Sheet(sheet) => {
                self.sheet = sheet;
                self.show_screen();
            }
            Press::Setting(setting) => self.set(setting),
            Press::Stop { step, theirs } => {
                let stops = &mut self.settings.stops;
                let set = if theirs {
                    &mut stops.theirs
                } else {
                    &mut stops.yours
                };
                *set = toggled(*set, step);
                self.save_stops();
            }
            Press::Concede => {
                if let Some(session) = self.session {
                    self.commands.push_back(SessionCommand::Concede(session));
                }
            }
            Press::Start => self.start_game(),
            Press::Agent(agent) => {
                self.start.agent = Some(agent);
                self.show_screen();
            }
            Press::FairDeal => {
                self.start.fair = !self.start.fair;
                self.show_screen();
            }
            Press::ExportBugs => self.requests.push_back(Request::ExportBugs),
            Press::Replay { recording } => self.replay(recording),
            Press::Seat(seat) => self.set_seat(seat),
            Press::Focus(id) => self.layer.focus(Some(id)),
            Press::Field(id, ref text) => self.keep_field(id, text),
            // No screen opens a menu or a viewer yet, so there is nothing for these to act on:
            // the screens that open them give them their arms.
            Press::Menu(..) | Press::Dismiss(_) => {}
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
        self.sheet = None;
        self.show_screen();
    }

    /// Clicks the open prompt, naming the decision and the prompt's state the click was made
    /// on. Does nothing while no prompt is open.
    fn click(&mut self, click: Click) {
        let Some((decision, prompt)) = &self.prompt else {
            return;
        };
        self.commands.push_back(SessionCommand::Click(ClickRequest {
            decision: *decision,
            revision: prompt.revision,
            click,
        }));
    }

    /// Changes a setting, asks the host to save the settings, and tells it the sound levels
    /// or the session the stops when those changed.
    fn set(&mut self, setting: Setting) {
        self.settings.apply(setting);
        self.requests
            .push_back(Request::SaveSettings(self.settings));
        match setting {
            Setting::Music(_) | Setting::Effects(_) | Setting::Muted(_) => {
                self.requests.push_back(Request::Audio {
                    music: self.settings.music,
                    effects: self.settings.effects,
                    muted: self.settings.muted,
                });
            }
            Setting::FullControl(_) => self.send_stops(),
        }
        self.show_screen();
    }

    /// Asks the host to save the settings after a stop changed, and sends the stops to the
    /// session.
    fn save_stops(&mut self) {
        self.requests
            .push_back(Request::SaveSettings(self.settings));
        self.send_stops();
        self.show_screen();
    }

    /// Sends the player's stops to the session of the latest view, if there is one.
    fn send_stops(&mut self) {
        if let Some(session) = self.session {
            self.commands
                .push_back(SessionCommand::SetStops(session, self.settings.stops));
        }
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
        self.show.rested()
    }

    /// When the first notice shown or to be shown ends, as of the last `frame`, if there is one
    /// (`Show::notice_end`).
    #[must_use]
    pub fn notice_end(&self) -> Option<u64> {
        self.show.notice_end()
    }

    /// The match's faces, which the host's asset workers prepare: each arrives as an
    /// `AssetReady::Face` naming its index in this list.
    #[must_use]
    pub fn faces(&self) -> &FaceList {
        self.show.faces().list()
    }

    /// The layer each face has landed in at each size.
    #[must_use]
    pub fn face_layers(&self) -> &Faces {
        self.show.faces()
    }

    /// The pointer on the table, for tests that read the held card's lean.
    #[must_use]
    pub fn pointer(&self) -> &crate::pointer::Pointer {
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

/// `set` with `step` added if it was not in it, or taken out if it was.
fn toggled(set: StepSet, step: beryl_protocol::protocol::Step) -> StepSet {
    let mut steps: Vec<_> = set.steps().collect();
    let step = step.into();
    match steps.iter().position(|&held| held == step) {
        Some(at) => {
            steps.remove(at);
        }
        None => steps.push(step),
    }
    StepSet::of(&steps)
}
