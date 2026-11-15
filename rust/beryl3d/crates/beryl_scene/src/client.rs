//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "Components" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

use std::collections::VecDeque;

use beryl_protocol::StartRequest;
use beryl_protocol::prompt::{Click, ClickReply, ClickRequest, PromptView};
use beryl_protocol::protocol::{DecisionId, SessionId, Status, StepSet};
use beryl_protocol::script::{OwnTimings, Verb, own_timings};

use crate::command::{Reply, Request, SessionCommand};
use crate::element::Press;
use crate::faces::{FaceList, Faces};
use crate::frame::{self, ElementId, Frame, FrameBuilder, Wake};
use crate::input::{AssetReady, Button, GlyphKey, Input, Pointer, PointerKind, PointerPhase};
use crate::layout::Layer;
use crate::play::Show;
use crate::play::record::{Plant, ScriptRecord};
use crate::prompts;
use crate::screens::{self, Music, Screen, ScreenIn, Sheet};
use crate::settings::{Setting, Settings};
use crate::table::{Table, Window};

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
    /// The screen the flat layer shows. A recording or a game is fed to the client on the game
    /// screen, which it starts on.
    screen: Screen,
    /// The sheet open over the screen, if any.
    sheet: Option<Sheet>,
    settings: Settings,
    /// The names of the agents a person may play, as the host listed them.
    agents: Vec<String>,
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
    was: Option<PromptView>,
    own_due: bool,
    /// Whether the prompt bar is drawn and takes presses. The bar of the latest view is laid out
    /// as the view arrives, because layout runs in `handle`, but it opens only once that view's
    /// script and every script before it have played out (`Show::settled`), as the web
    /// interface's prompt opens when its view has played out. Set by each `frame`, so a press
    /// reaches only a bar the last frame drew.
    open: bool,
    /// The element the primary pointer went down on, which a release on it presses.
    pressed: Option<ElementId>,
    /// Where the mouse pointer is, in logical points, while it is over the window. The element
    /// a press there would reach is drawn `hovered`. A touch never hovers.
    mouse: Option<glam::Vec2>,
    wake: Wake,
    requests: VecDeque<Request>,
    commands: VecDeque<SessionCommand>,
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
            screen: Screen::Game,
            sheet: None,
            settings: Settings::default(),
            agents: Vec::new(),
            session: None,
            music: None,
            own_timings: own_timings(),
            prompt: None,
            was: None,
            own_due: false,
            open: false,
            pressed: None,
            mouse: None,
            wake: Wake::Never,
            requests: VecDeque::new(),
            commands: VecDeque::new(),
        }
    }

    /// Takes a session update, a pointer or key event, a new surface or another input. An
    /// input has no time of its own: what it starts begins at the `now` of the next `frame`.
    /// This is where screens are rebuilt and laid out, and where text is shaped.
    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Update(update) => {
                self.session = Some(update.session);
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
                self.show_screen();
            }
            Input::Settings(settings) => {
                self.settings = settings;
                self.show_screen();
            }
            Input::Agents(agents) => {
                self.agents = agents;
                self.show_screen();
            }
            Input::Pointer(pointer) => self.point(pointer),
            Input::Cards(records) => {
                *self.show.faces_mut() = Faces::new(FaceList::from_records(&records));
            }
            Input::Assets(AssetReady::Face { face, size, layer }) => {
                self.show.faces_mut().arrived(face, size, layer);
            }
            Input::Assets(AssetReady::Glyphs { page, glyphs }) => {
                self.layer.glyphs_mut().add(page, &glyphs);
            }
            Input::Surface {
                size,
                scale,
                safe_area,
            } => {
                self.window = Window {
                    size,
                    scale,
                    safe_area,
                };
                self.show.resize(self.window);
                self.layer.resize(self.window);
            }
            input => self.inputs.push(input),
        }
        self.wake = Wake::Now;
    }

    /// The frame at `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, now: u64) -> &Frame {
        self.inputs.clear();
        if self.own_due {
            self.own_due = false;
            if let Some((_, prompt)) = &self.prompt {
                let lit = self.show.lit_mut();
                prompts::own(prompt, self.was.as_ref(), lit, &self.own_timings, now);
            }
        }
        let mut builder = FrameBuilder::new(&mut self.frame);
        self.show.frame(now, &mut builder);
        self.wake = self.show.wake(now);
        let (layout_nodes, text_runs) = self.layer.take_work();
        builder.layout_work(layout_nodes, text_runs);
        builder.placement_work(self.show.table().take_placed());
        self.open = self.show.settled();
        (self.layer).write(&mut builder, self.show.table().camera(), self.open);
        builder.finish();
        if let Some(point) = self.mouse
            && let Some(index) = crate::agent::hit(&self.frame, point)
        {
            self.frame.resolved[index as usize].state.0 |= frame::StateBits::HOVERED;
        }
        &self.frame
    }

    /// The spans and streaks the table shows, for the marks and arrows a prompt begins and ends
    /// itself, timed by the session's `own_timings`.
    pub fn lit_mut(&mut self) -> &mut crate::light::Lit {
        self.show.lit_mut()
    }

    /// Takes `prompt` as the open prompt. If it differs from the one open before, the next
    /// `frame` calls `prompts::own` for the change. A prompt closing makes no call, because
    /// `prompts::own` is given the open prompt.
    fn set_prompt(&mut self, prompt: Option<(DecisionId, PromptView)>) {
        let changed = self.prompt.as_ref().map(|(_, prompt)| prompt)
            != prompt.as_ref().map(|(_, prompt)| prompt);
        let was = std::mem::replace(&mut self.prompt, prompt);
        if changed {
            // A change the next frame has not yet passed to `prompts::own` keeps the state
            // before it, which is the last one `prompts::own` saw.
            if !self.own_due {
                self.was = was.map(|(_, prompt)| prompt);
            }
            self.own_due = self.prompt.is_some();
        }
    }

    /// Rebuilds the flat layer's screen, or the sheet open over it, from what the client holds
    /// now, and asks the host for the music it calls for if that has changed.
    fn show_screen(&mut self) {
        let view = self.show.latest().map(|update| &update.view);
        let input = ScreenIn {
            window: self.window,
            view,
            prompt: self.prompt.as_ref().map(|(_, prompt)| prompt),
            settings: &self.settings,
            hovered: None,
            sheet: self.sheet,
            agents: &self.agents,
        };
        // Until the sheets are built, an open sheet takes the screen's place in the flat layer.
        // Until the game screen shows anything besides the open prompt, it shows nothing while
        // no prompt is open, so that no empty panel is drawn over the table.
        let shown = match self.sheet {
            Some(sheet) => Some(screens::sheet(sheet, &input)),
            None if self.screen == Screen::Game && input.prompt.is_none() => None,
            None => Some(screens::screen(self.screen, &input)),
        };
        let music = screens::music(self.screen, view);
        self.layer.show(shown);
        if music != self.music {
            self.music = music;
            self.requests.push_back(Request::Music(music));
        }
    }

    /// A press is the primary button or a touch going down and coming up on the same enabled
    /// element. It queues the command the element's `Press` means.
    fn point(&mut self, pointer: Pointer) {
        if pointer.kind == PointerKind::Mouse {
            self.mouse = match pointer.phase {
                PointerPhase::Cancel | PointerPhase::Leave => None,
                _ => Some(pointer.position),
            };
        }
        let primary = pointer.button == Button::Primary;
        let open = self.open;
        match pointer.phase {
            PointerPhase::Down if primary => {
                self.pressed = (self.layer.press_at(pointer.position, open)).map(|(id, ..)| id);
            }
            PointerPhase::Up if primary => {
                let pressed = self.pressed.take();
                if let Some((id, press, true)) = self.layer.press_at(pointer.position, open)
                    && pressed == Some(id)
                {
                    self.press(&press);
                }
            }
            PointerPhase::Cancel | PointerPhase::Leave => self.pressed = None,
            _ => {}
        }
    }

    /// Carries out what a press on an element means: a command for the session, a request of
    /// the host, or a change of screen, sheet or settings. Every `Press` is interpreted here
    /// and nowhere else.
    pub fn press(&mut self, press: &Press) {
        match *press {
            Press::Button(button) => self.click(Click::Button { button }),
            Press::Prompt(click) => self.click(click),
            Press::Open(screen) => {
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
            Press::Start { agent } => {
                if let Some(name) = self.agents.get(usize::from(agent)) {
                    self.commands.push_back(SessionCommand::Start(StartRequest {
                        agent: name.clone(),
                        seed: None,
                        fair: false,
                    }));
                    self.screen = Screen::Game;
                    self.sheet = None;
                    self.show_screen();
                }
            }
            // No menu, text field or viewer is built yet, so there is nothing for these to act
            // on: the widgets and game screen tracks give them their arms.
            Press::Menu(..) | Press::Focus(_) | Press::Dismiss(_) => {}
        }
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

    /// The match's faces, which the host's asset workers prepare: each arrives as an
    /// `AssetReady::Face` naming its index in this list.
    #[must_use]
    pub fn faces(&self) -> &FaceList {
        self.show.faces().list()
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
