//! The test driver: a recording played through the client inside the test process, driven with
//! locators and actions in the manner of Playwright (docs/beryl-3d.md, "The test driver").
//!
//! The test owns the clock, so nothing waits by itself: an action happens at the clock's
//! present time, and the clock moves only in `step`, `still` and `seek`. A recording plays on
//! a clock of its own: each recorded reply is given 400 milliseconds after the table comes to
//! rest in the view it answers, so every moment has the same time on every run. A test that
//! acts first answers the view itself, from the recording's responses.
//!
//! This driver uses the same client, inputs and replies as the app.

mod check;
mod cues;
mod gesture;
mod glyphs;
mod keys;
mod playthrough;
mod recording;

use crate::client;
use client::{Client, Config, Input, SessionCommand};
use std::fmt::{self, Write};
use std::sync::Arc;

use kit_base::data::Data;

use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Verb;
use glam::Vec2;

pub use check::{Checked, Mode, StepMoment, TimelineStep};
pub use game_probe::driver::{
    Action, Duration, Element, KeyChord, Moment, Name, OPEN, Offset, ParseError, RECORDINGS,
    REPLY_DELAY, TICK, key_code, plays_on, point,
};
pub use recording::{Answer, Recording, Responses};

use self::cues::Cues;
use game_play::play::record::Plant;
use game_table::faces::FaceList;
use kit_frame::frame::{Frame, Sound, StateBits, Wake};
use kit_probe::agent::checks;
use kit_probe::agent::tree::locator_of;
use kit_probe::agent::{Fault, Layout, TreeOptions, locate, tree};
use kit_probe::driver::{moment_distance, offset_from};
use kit_scene::hit::{bounds, uncovered};
use kit_ui::input::{
    AssetReady, Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size, Tier,
};

/// How long `still` runs the clock before it gives up on the table coming to rest.
const STILL_LIMIT: Duration = Duration::from_millis(600_000);
/// How many moves a drag makes between its press and its release, and the step after each.
const DRAG_MOVES: u32 = 8;
const DRAG_STEP: Duration = Duration::from_millis(16);

/// Why the driver could not do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverError(pub String);

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DriverError {}

/// A recording played through a client, for one seat in one window.
pub struct Page {
    /// The recording and what is read from it, shared with the pages `fresh` makes, which
    /// play it again: copying them for each play took about a fifth of a test's time.
    recording: Arc<Recording>,
    views: Arc<[GameUpdate]>,
    /// The card records the recording starts with, from which the client lists its faces.
    cards: Option<Arc<CardRecords>>,
    responses: Responses,
    name: String,
    seat: Side,
    layout: Layout,
    size: Size,
    /// Physical pixels to a logical point, as the client is told ([`Page::with_scale`]).
    scale: f32,
    /// The window's safe area, for the client and the checks alike ([`Page::with_safe_area`]).
    safe_area: Insets,
    /// The visual tier the client is told of whenever it is made anew ([`Page::with_tier`]).
    tier: Tier,
    tick: Duration,
    cues: Cues,
    client: Client,
    /// The clock, in nanoseconds since the recording's start.
    now: u64,
    /// How many views the client has been given.
    released: usize,
    /// The index of the first view of the latest batch given together.
    batch: usize,
    /// Since when the client has been at rest with nothing left to do, if it is.
    rest_since: Option<u64>,
    /// Every input handed to the client, in order, and the time of each.
    inputs: Vec<Input>,
    given_at: Vec<u64>,
    /// Whether the client keeps a record of every script, for the interface checks.
    records: bool,
    /// The faults planted in the recording's scripts (`plant`).
    plants: Vec<Plant>,
    /// Whether scripts play faster while many updates wait (`Client::set_catch_up`).
    catch_up: bool,
    /// Each named moment of the recording, with its time, once asked for.
    moments: Option<Vec<(Name, u64)>>,
    /// Every step of the recording's scripts, once asked for.
    steps: Option<Vec<StepMoment>>,
    heard: Vec<(u64, Sound)>,
    /// The glyph pages a host has given (`give_glyphs`), given again whenever the client is
    /// made anew, so that its text keeps drawing after a seek back.
    glyphs: Vec<AssetReady>,
    /// Whether the driver answers every glyph and icon the client asks for itself, as it does
    /// unless a host that draws gives the glyphs ([`Page::with_host_atlas`]).
    stub_atlas: bool,
}

impl Page {
    /// The recording at its start, as `seat` sees it in a window of `size` logical points.
    ///
    /// # Errors
    ///
    /// When a view in the recording cannot be read.
    pub fn new(
        data: Arc<Data>,
        recording: Arc<Recording>,
        seat: Side,
        layout: Layout,
        size: Size,
    ) -> Result<Self, DriverError> {
        let views = recording.views().map_err(DriverError)?.into();
        let cards = recording.card_records().map_err(DriverError)?.map(Arc::new);
        let mut page = Self {
            responses: Responses::shared(Arc::clone(&recording)),
            recording,
            views,
            cards,
            name: "recording".to_owned(),
            seat,
            layout,
            size,
            scale: 1.0,
            safe_area: Insets::default(),
            tier: Tier::Baseline,
            tick: TICK,
            cues: Cues::default(),
            client: Client::new(data, Config::LARGEST),
            now: 0,
            released: 0,
            batch: 0,
            rest_since: None,
            inputs: Vec::new(),
            given_at: Vec::new(),
            records: false,
            plants: Vec::new(),
            catch_up: true,
            moments: None,
            steps: None,
            heard: Vec::new(),
            glyphs: Vec::new(),
            stub_atlas: true,
        };
        page.begin();
        Ok(page)
    }

    /// The same page, naming its recording `name` in faults and errors.
    #[must_use]
    pub fn named(mut self, name: &str) -> Self {
        name.clone_into(&mut self.name);
        self
    }

    /// The same page at its start, in a window whose safe area starts `safe_area` in from each
    /// edge: the client lays out within it and the interface checks hold it to the same insets.
    #[must_use]
    pub fn with_safe_area(mut self, safe_area: Insets) -> Self {
        self.safe_area = safe_area;
        self.restart();
        self
    }

    /// The same page at its start, in a window of `scale` physical pixels to a logical point, as
    /// a Retina display is at 2. The client lays out in logical points still, so where every
    /// element is stays the same, but it asks for its glyphs at that scale and gives its clips
    /// in physical pixels, as it does in the desktop app.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        assert!(scale.is_finite() && scale > 0.0, "a scale is positive");
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.restart();
        }
        self
    }

    /// Physical pixels to a logical point ([`Page::with_scale`]).
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The same page at its start, drawn in `tier`: the client leaves out the records that tier
    /// does not draw. Where everything is stays the same.
    #[must_use]
    pub fn with_tier(mut self, tier: Tier) -> Self {
        if tier != self.tier {
            self.tier = tier;
            self.restart();
        }
        self
    }

    /// Sets the length of a tick of the clock.
    pub fn set_tick(&mut self, tick: Duration) {
        assert!(tick.0 > 0, "a tick has a length");
        self.tick = tick;
    }

    /// The clock, in nanoseconds since the recording's start.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Whether the recording has played to its end: every view has been given and the table
    /// rests in the last.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.released >= self.views.len() && self.rest_since.is_some() && self.cues.done()
    }

    /// The next verb the client played as an instant change because it is not built yet, each
    /// verb returned once.
    pub fn next_unbuilt(&mut self) -> Option<Verb> {
        self.client.next_unbuilt()
    }

    /// The match's faces, as the client listed them from the recording's card records: what a
    /// host hands its asset workers (`Client::faces`).
    #[must_use]
    pub fn faces(&self) -> &FaceList {
        self.client.faces()
    }

    /// When the client was given the latest view, in nanoseconds since the recording's start.
    #[must_use]
    pub fn view_given_at(&self) -> Option<u64> {
        (self.inputs.iter().zip(&self.given_at).rev())
            .find(|(input, _)| matches!(input, Input::Update(_)))
            .map(|(_, &at)| at)
    }

    /// The page with scripts played at real time however many updates wait, unless `catch_up`;
    /// it starts again if that changes.
    #[must_use]
    pub fn with_catch_up(mut self, catch_up: bool) -> Self {
        if catch_up != self.catch_up {
            self.catch_up = catch_up;
            self.restart();
        }
        self
    }

    /// The window the page lays its frames out in, in logical points.
    #[must_use]
    pub fn size(&self) -> Size {
        self.size
    }

    /// Whether later frames record their elements' looks (`Client::set_looking`), until a restart.
    pub fn set_looking(&mut self, looking: bool) {
        self.client.set_looking(looking);
    }

    /// Every input handed to the client so far, in order.
    #[must_use]
    pub fn inputs(&self) -> &[Input] {
        &self.inputs
    }

    /// The frame at the present time. With a stub atlas, every glyph its text asked for is
    /// delivered first.
    pub fn frame(&mut self) -> &Frame {
        self.render();
        if self.stub_atlas {
            self.deliver_stub_glyphs();
        }
        self.client.frame(self.now)
    }

    /// Prints the current state of a registered feature, by id.
    #[must_use]
    pub fn feature_state(&self, id: &str) -> Option<String> {
        self.client.feature_state(id)
    }

    /// The tree printout of the present frame.
    pub fn tree(&mut self) -> String {
        let frame = self.frame();
        tree(frame, &TreeOptions::default()).expect("the whole tree needs no locator")
    }

    /// The element `locator` names in the present frame.
    ///
    /// # Panics
    ///
    /// When the locator names no element or several, with the nearest names and the clock's state.
    pub fn find(&mut self, locator: &str) -> Element {
        self.try_find(locator)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The element `locator` names in the present frame.
    ///
    /// # Errors
    ///
    /// When the locator names no element or several; the message ends with the clock's state.
    pub fn try_find(&mut self, locator: &str) -> Result<Element, DriverError> {
        let frame = self.frame();
        match locate(frame, locator) {
            Ok(index) => {
                let element = &frame.resolved[index as usize];
                Ok(Element {
                    index,
                    role: element.role,
                    name: frame.name(element.name).to_owned(),
                    place: element.place,
                    rect: bounds(element),
                    interactive: element.interactive,
                    state: element.state,
                })
            }
            Err(error) => Err(self.error(&error.to_string())),
        }
    }

    /// The faults every interface check finds in the present frame.
    pub fn faults(&mut self) -> Vec<Fault> {
        let context = self.context();
        checks::check(self.frame(), &context)
    }

    /// Runs every interface check on the present frame.
    ///
    /// # Panics
    ///
    /// When a check finds a fault, with every fault and the clock's state.
    pub fn check(&mut self) {
        let faults = self.faults();
        if faults.is_empty() {
            return;
        }
        let context = self.context();
        let printed: Vec<String> = faults.iter().map(|fault| fault.print(&context)).collect();
        panic!("{}", self.error(&printed.join("\n")));
    }

    /// The tuning the page's client was made with.
    #[must_use]
    pub fn data(&self) -> &Arc<Data> {
        self.client.data()
    }

    /// The spans and streaks the table shows now: the marks and arrows among them.
    #[must_use]
    pub fn lit(&self) -> &game_play::light::Lit {
        self.client.lit()
    }

    /// Presses and releases the primary button where `locator`'s element is uncovered.
    pub fn press(&mut self, locator: &str) {
        self.click(locator, Button::Primary);
    }

    /// Presses and releases `button` where `locator`'s element is uncovered: the secondary
    /// button is a right click, which the `menu` action gives.
    pub fn click(&mut self, locator: &str, button: Button) {
        let point = self.point(locator);
        self.pointer(PointerPhase::Down, point, button);
        self.pointer(PointerPhase::Up, point, button);
        self.render();
    }

    /// Moves the pointer to where `locator`'s element is uncovered.
    pub fn hover(&mut self, locator: &str) {
        let point = self.point(locator);
        self.pointer(PointerPhase::Move, point, Button::None);
        self.render();
    }

    /// Presses on `from`'s element, moves in eight even steps along the straight line to `to`'s
    /// element with a 16 ms step after each, and releases there.
    pub fn drag(&mut self, from: &str, to: &str) {
        let (start, end) = (self.point(from), self.point(to));
        self.drag_between(start, end);
    }

    /// Moves the pointer to `point`, in logical points.
    pub fn move_to(&mut self, point: Vec2) {
        self.pointer(PointerPhase::Move, point, Button::None);
        self.render();
    }

    /// Runs the clock for `duration`, giving each recorded reply when it is due.
    pub fn step(&mut self, duration: Duration) {
        self.advance_to(self.now + duration.0);
    }

    /// Plays `script`, one action a line, through the same methods. Blank lines and lines that
    /// start with `#` are skipped.
    ///
    /// # Panics
    ///
    /// When a line is not an action, or when an action fails.
    pub fn run(&mut self, script: &str) {
        for line in script.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let action: Action = line.parse().unwrap_or_else(|error| panic!("{error}"));
            self.act(&action);
        }
    }

    /// Does one action, or says why it cannot be done without doing it: a locator that names
    /// no element or several, or an element that is covered.
    ///
    /// # Errors
    ///
    /// When a locator of the action names no single uncovered element in the present frame.
    pub fn try_act(&mut self, action: &Action) -> Result<(), DriverError> {
        let locators: Vec<&str> = match action {
            Action::Press(locator)
            | Action::Hover(locator)
            | Action::Menu(locator)
            | Action::Type(locator, _)
            | Action::Wheel(locator, _) => vec![locator],
            Action::Drag(from, to) | Action::Hold(from, to) => vec![from, to],
            Action::Key(_) | Action::Move(_) | Action::Step(_) | Action::Still => Vec::new(),
        };
        for locator in locators.into_iter().filter(|text| point(text).is_none()) {
            let element = self.try_find(locator)?;
            if uncovered(self.frame(), element.index).is_none() {
                let message = format!("no part of '{locator}' can be pressed: it is covered");
                return Err(self.error(&message));
            }
        }
        self.act(action);
        Ok(())
    }

    /// Where the page is: how many views the client has been given, of how many, and whether
    /// anything on the table is moving.
    pub fn position(&mut self) -> (usize, usize, bool) {
        self.render();
        (
            self.released,
            self.views.len(),
            self.client.wake() != Wake::Never,
        )
    }

    /// Does one action.
    pub fn act(&mut self, action: &Action) {
        match action {
            Action::Press(locator) => self.press(locator),
            Action::Hover(locator) => self.hover(locator),
            Action::Menu(locator) => self.click(locator, Button::Secondary),
            Action::Drag(from, to) => self.drag(from, to),
            Action::Hold(from, to) => {
                let (start, end) = (self.point(from), self.point(to));
                self.hold_between(start, end);
            }
            Action::Type(locator, text) => self.type_text(locator, text),
            Action::Key(chord) => self.key_chord(*chord),
            Action::Move(point) => self.move_to(*point),
            Action::Wheel(locator, by) => {
                let point = self.point(locator);
                self.wheel(point, *by);
            }
            Action::Step(duration) => self.step(*duration),
            Action::Still => self.still(),
        }
    }

    /// Every named moment of the recording with its time, as the recording plays on its own
    /// clock with no action taken. Views given together, because the game played on
    /// without the viewer, share the moment the last of them comes to rest. Each view's steps
    /// are listed after it, though they play before it rests, each at the middle of its span as
    /// the client's script clock played it.
    pub fn moments(&mut self) -> Vec<(Name, Duration)> {
        self.named_moments()
            .iter()
            .map(|&(name, time)| (name, Duration(time)))
            .collect()
    }

    /// Goes to `moment`, starting the recording over first when it lies in the past.
    ///
    /// # Errors
    ///
    /// When the recording has no such moment.
    pub fn seek(&mut self, moment: &Moment) -> Result<(), DriverError> {
        let time = self.time_of(moment)?;
        if time < self.now {
            self.restart();
        }
        self.advance_to(time);
        Ok(())
    }

    /// The time of `moment`, in nanoseconds since the recording's start.
    ///
    /// # Errors
    ///
    /// When the recording has no such moment.
    pub fn time_of(&mut self, moment: &Moment) -> Result<u64, DriverError> {
        let (name, offset) = match *moment {
            Moment::At(duration) => return Ok(duration.0),
            Moment::Named { name, offset } => (name, offset),
        };
        let moments = self.named_moments();
        let Some(&(_, time)) = moments.iter().find(|(known, _)| *known == name) else {
            // The five nearest: moments of the same kind first, then by their numbers.
            let mut near: Vec<Name> = moments.iter().map(|&(known, _)| known).collect();
            near.sort_by_key(|&known| moment_distance(known, name));
            near.truncate(5);
            let names: Vec<String> = near.iter().map(ToString::to_string).collect();
            return Err(DriverError(format!(
                "the recording has no moment '{name}'; the nearest are {}",
                names.join(", ")
            )));
        };
        Ok(offset_from(time, offset))
    }

    /// The clock's state, which ends every failure's message: the time, the view, and whether
    /// anything is moving.
    pub fn clock(&mut self) -> String {
        self.render();
        let mut out = format!(
            "t={} view {} of {}",
            Duration(self.now),
            self.released,
            self.views.len()
        );
        if self.client.wake() == Wake::Never {
            out.push_str(", at rest");
            return out;
        }
        let frame = self.client.frame(self.now);
        let moving: Vec<String> = (0..frame.resolved.len() as u32)
            .filter(|&index| frame.resolved[index as usize].state.has(StateBits::MOVING))
            .take(3)
            .map(|index| locator_of(frame, index))
            .collect();
        out.push_str(", animating");
        if !moving.is_empty() {
            let _ = write!(out, " ({})", moving.join(", "));
        }
        out.push_str(": do the `still` action first? (`--do still` in `just b3d`)");
        out
    }

    fn error(&mut self, message: &str) -> DriverError {
        DriverError(format!("{message}\n{}", self.clock()))
    }

    fn pointer(&mut self, phase: PointerPhase, position: Vec2, button: Button) {
        self.give(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position,
            button,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }

    fn give(&mut self, input: Input) {
        self.inputs.push(input.clone());
        self.given_at.push(self.now);
        self.client.handle(input);
    }

    /// Gives the client the recording's initial inputs and views. Text tools supply stub atlas
    /// cells and face layers. A host that draws supplies the actual uploads.
    fn begin(&mut self) {
        if self.records {
            self.client.record();
        }
        self.client.set_catch_up(self.catch_up);
        for plant in &self.plants {
            self.client.plant(*plant);
        }
        if self.tier != Tier::Baseline {
            self.give(Input::Tier(self.tier));
        }
        self.give(Input::Surface {
            size: self.size,
            scale: self.scale,
            safe_area: self.safe_area,
        });
        if let Some(cards) = &self.cards {
            self.give(Input::Cards(Box::new(CardRecords::clone(cards))));
            if self.stub_atlas {
                let ready: Vec<AssetReady> = self.client.faces().all_ready().collect();
                for ready in ready {
                    self.give(Input::Assets(ready));
                }
            }
        }
        if self.stub_atlas {
            self.give(Input::Assets(glyphs::stub_client_icons(self.scale)));
        }
        for ready in self.glyphs.clone() {
            self.give(Input::Assets(ready));
        }
        if let Some(names) = self.recording.recordings() {
            self.give(Input::Recordings(names));
        }
        for press in self.recording.opens() {
            self.client.press(&press);
        }
        for lobby in self.recording.lobbies() {
            self.give(Input::Lobby(Box::new(lobby)));
        }
        if self.stub_atlas && self.client.showing_gallery() {
            let ready: Vec<_> = self.client.faces().all_ready().collect();
            for asset in ready {
                if let AssetReady::Face { face, layer, .. } = asset {
                    self.give(Input::Assets(AssetReady::GalleryFace { face, layer }));
                }
            }
        }
        self.release();
        // After the first views, as the live app sends them once it has resumed a game, so
        // that a recording of a game stays on the game screen and one with no views shows the
        // start screen. Every recording the web's fixtures share answers `list_agents`. The
        // saved tables come first, as the live app sends them, so that the start screen's
        // first frame lists them.
        if let Ok(Some(agents)) = self.recording.agents() {
            if let Ok(saved) = self.responses.answer(&SessionCommand::TableSaved) {
                self.give(Input::Reply(saved.reply));
            }
            self.give(Input::Agents(agents));
        }
        self.render();
    }

    fn restart(&mut self) {
        self.client = Client::new(Arc::clone(self.client.data()), Config::LARGEST);
        self.responses = Responses::shared(Arc::clone(&self.recording));
        self.now = 0;
        self.released = 0;
        self.batch = 0;
        self.rest_since = None;
        self.cues.next = 0;
        self.inputs.clear();
        self.given_at.clear();
        self.begin();
    }

    /// Gives the client the next view, and every view after it that follows unasked.
    fn release(&mut self) {
        self.batch = self.released;
        while let Some(update) = self.views.get(self.released) {
            let update = update.clone();
            self.released += 1;
            let unasked = plays_on(&update);
            self.give(Input::Update(Box::new(update)));
            if !unasked {
                break;
            }
        }
        self.rest_since = None;
    }

    /// Computes the frame at the present time, and answers every command it sends from the
    /// recording, until it sends none.
    fn render(&mut self) {
        self.give_cues();
        loop {
            self.client.prepare_scroll(self.now);
            let sounds = &self.client.frame(self.now).sounds;
            self.heard.extend(sounds.iter().map(|s| (self.now, *s)));
            let mut answered = false;
            while let Some(command) = self.client.next_command() {
                let answer = match self.responses.answer(&command) {
                    Ok(answer) => answer,
                    Err(message) => panic!("{}", self.error(&message)),
                };
                self.give(Input::Reply(answer.reply));
                if answer.plays_on {
                    self.release();
                }
                answered = true;
            }
            if !answered {
                break;
            }
        }
        if self.client.wake() == Wake::Never {
            // When the last script ended, not when this frame found it ended, so that a reply is
            // given at the same time however often the clock was stopped.
            let rested = self.client.rested();
            self.rest_since.get_or_insert(rested);
        } else {
            self.rest_since = None;
        }
    }

    /// When the recorded reply to the present view is due, if one is.
    fn reply_due(&self) -> Option<u64> {
        if self.released >= self.views.len() {
            return None;
        }
        self.rest_since.map(|rest| rest + self.reply_delay().0)
    }

    /// Runs the clock to `target`, giving each recorded reply when it is due.
    fn advance_to(&mut self, target: u64) {
        self.render();
        loop {
            if let Some(due) = self.reply_due()
                && due <= self.now
            {
                self.release();
                self.render();
                continue;
            }
            if self.now >= target {
                return;
            }
            self.now = self
                .next_frame()
                .min(self.reply_due().unwrap_or(u64::MAX))
                .min(target);
            self.render();
        }
    }

    fn named_moments(&mut self) -> &[(Name, u64)] {
        if self.moments.is_none() {
            let mut playing = self.fresh(true);
            self.moments = Some(playing.play_through());
        }
        self.moments.as_deref().expect("just set")
    }

    /// The same recording at its start, with the same faults planted and no action taken, the
    /// client keeping a record of its scripts if `records`.
    fn fresh(&self, records: bool) -> Self {
        let mut page = Self {
            recording: Arc::clone(&self.recording),
            views: Arc::clone(&self.views),
            cards: self.cards.clone(),
            responses: Responses::shared(Arc::clone(&self.recording)),
            name: self.name.clone(),
            seat: self.seat,
            layout: self.layout,
            size: self.size,
            scale: self.scale,
            safe_area: self.safe_area,
            tier: self.tier,
            tick: self.tick,
            cues: self.cues.fresh(),
            client: Client::new(Arc::clone(self.client.data()), Config::LARGEST),
            now: 0,
            released: 0,
            batch: 0,
            rest_since: None,
            inputs: Vec::new(),
            given_at: Vec::new(),
            records,
            plants: self.plants.clone(),
            catch_up: self.catch_up,
            moments: Some(Vec::new()),
            steps: Some(Vec::new()),
            heard: Vec::new(),
            glyphs: self.glyphs.clone(),
            stub_atlas: self.stub_atlas,
        };
        page.begin();
        page
    }
}
