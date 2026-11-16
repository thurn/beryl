//! The test driver: a recording played through the client inside the test process, driven with
//! locators and actions in the manner of Playwright (docs/beryl-3d.md, "The test driver").
//!
//! The test owns the clock, so nothing waits by itself: an action happens at the clock's
//! present time, and the clock moves only in `step`, `still` and `seek`. A recording plays on
//! a clock of its own: each recorded reply is given 400 milliseconds after the table comes to
//! rest in the view it answers, so every moment has the same time on every run. A test that
//! acts first answers the view itself, from the recording's responses.
//!
//! Only a driver of the client needs this (the `driver` feature); the app does not enable it.

mod action;
mod check;
mod cues;
mod gesture;
mod glyphs;
mod keys;
mod moment;
mod recording;

use std::fmt::{self, Write};
use std::sync::Arc;

use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::protocol::{Side, Status, ZoneName};
use beryl_protocol::script::Verb;
use glam::Vec2;

pub use action::{Action, key_code};
pub use check::{Checked, Mode, StepMoment, TimelineStep};
pub use moment::{Duration, Moment, Name, Offset, ParseError};
pub use recording::{Answer, OPEN, RECORDINGS, Recording, Responses, plays_on};

use self::cues::Cues;
use crate::agent::{self, Context, Fault, Layout, TreeOptions, locate, uncovered};
use crate::faces::FaceList;
use crate::frame::{Frame, Place, Role, Sound, StateBits, Wake};
use crate::input::{
    AssetReady, Button, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Rect, Size,
};
use crate::play::record::Plant;
use crate::{Client, Config};

/// How long after the table comes to rest a recorded reply is given.
pub const REPLY_DELAY: Duration = Duration::from_millis(400);
/// A tick of the test's clock unless the test sets another: a sixtieth of a second.
pub const TICK: Duration = Duration(16_666_667);
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

/// An element as `find` found it in the present frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// Its index in the frame's resolved elements.
    pub index: u32,
    pub role: Role,
    pub name: String,
    pub place: Place,
    pub rect: Rect,
    pub interactive: bool,
    pub state: StateBits,
}

impl Element {
    /// The zone a card is in; `None` for an element of the flat layer.
    #[must_use]
    pub fn zone(&self) -> Option<ZoneName> {
        match self.place {
            Place::Card { zone, .. } => Some(zone),
            Place::Layer => None,
        }
    }

    /// Its bounding box on screen, in logical points.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    #[must_use]
    pub fn states(&self) -> StateBits {
        self.state
    }
}

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
    /// Each named moment of the recording, with its time, once asked for.
    moments: Option<Vec<(Name, u64)>>,
    /// Every step of the recording's scripts, once asked for.
    steps: Option<Vec<StepMoment>>,
    heard: Vec<(u64, Sound)>,
    /// The glyph pages a host has given (`give_glyphs`), given again whenever the client is
    /// made anew, so that its text keeps drawing after a seek back.
    glyphs: Vec<AssetReady>,
}

impl Page {
    /// The recording at its start, as `seat` sees it in a window of `size` logical points.
    ///
    /// # Errors
    ///
    /// When a view in the recording cannot be read.
    pub fn new(
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
            tick: TICK,
            cues: Cues::default(),
            client: Client::new(Config::LARGEST),
            now: 0,
            released: 0,
            batch: 0,
            rest_since: None,
            inputs: Vec::new(),
            given_at: Vec::new(),
            records: false,
            plants: Vec::new(),
            moments: None,
            steps: None,
            heard: Vec::new(),
            glyphs: Vec::new(),
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

    /// Every input handed to the client so far, in order.
    #[must_use]
    pub fn inputs(&self) -> &[Input] {
        &self.inputs
    }

    /// The frame at the present time.
    pub fn frame(&mut self) -> &Frame {
        self.render();
        self.client.frame(self.now)
    }

    /// The tree printout of the present frame.
    pub fn tree(&mut self) -> String {
        let frame = self.frame();
        agent::tree(frame, &TreeOptions::default()).expect("the whole tree needs no locator")
    }

    /// The element `locator` names in the present frame.
    ///
    /// # Panics
    ///
    /// When the locator names no element or several, with the nearest names and the clock's
    /// state.
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
                    rect: agent::hit::bounds(element),
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
        agent::check(self.frame(), &context)
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

    /// The spans and streaks the table shows now: the marks and arrows among them.
    #[must_use]
    pub fn lit(&self) -> &crate::light::Lit {
        self.client.lit()
    }

    /// Presses and releases the primary button where `locator`'s element is uncovered.
    pub fn press(&mut self, locator: &str) {
        let point = self.point(locator);
        self.pointer(PointerPhase::Down, point, Button::Primary);
        self.pointer(PointerPhase::Up, point, Button::Primary);
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
            Action::Press(locator) | Action::Hover(locator) | Action::Type(locator, _) => {
                vec![locator]
            }
            Action::Drag(from, to) => vec![from, to],
            Action::Key(_) | Action::Move(_) | Action::Step(_) | Action::Still => Vec::new(),
        };
        for locator in locators {
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
            Action::Drag(from, to) => self.drag(from, to),
            Action::Type(locator, text) => self.type_text(locator, text),
            Action::Key(code) => self.key(*code),
            Action::Move(point) => self.move_to(*point),
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
            .map(|index| agent::tree::locator_of(frame, index))
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

    fn context(&self) -> Context {
        Context {
            recording: self.name.clone(),
            seat: self.seat,
            layout: self.layout,
            size: self.size,
            safe_area: Insets::default(),
            at: self.now,
        }
    }

    /// Where an action on `locator`'s element happens: the centre of its largest uncovered
    /// rectangle.
    fn point(&mut self, locator: &str) -> Vec2 {
        let element = self.find(locator);
        let frame = self.frame();
        let Some(rect) = uncovered(frame, element.index) else {
            let message = format!("no part of '{locator}' can be pressed: it is covered");
            panic!("{}", self.error(&message));
        };
        Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
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

    /// Sets the client up and gives it the recording's first views. The driver acts as a host
    /// whose asset workers have every face ready at the start: it gives the client the
    /// recording's card records, then every face of the list the client made from them
    /// (`FaceList::all_ready`).
    fn begin(&mut self) {
        if self.records {
            self.client.record();
        }
        for plant in &self.plants {
            self.client.plant(*plant);
        }
        self.give(Input::Surface {
            size: self.size,
            scale: 1.0,
            safe_area: Insets::default(),
        });
        if let Some(cards) = &self.cards {
            self.give(Input::Cards(Box::new(CardRecords::clone(cards))));
            let ready: Vec<AssetReady> = self.client.faces().all_ready().collect();
            for ready in ready {
                self.give(Input::Assets(ready));
            }
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
        self.release();
        // After the first views, as the live app sends them once it has resumed a game, so
        // that a recording of a game stays on the game screen and one with no views shows the
        // start screen. Every recording the web's fixtures share answers `list_agents`.
        if let Ok(Some(agents)) = self.recording.agents() {
            self.give(Input::Agents(agents));
        }
        self.render();
    }

    fn restart(&mut self) {
        self.client = Client::new(Config::LARGEST);
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
            tick: self.tick,
            cues: self.cues.fresh(),
            client: Client::new(Config::LARGEST),
            now: 0,
            released: 0,
            batch: 0,
            rest_since: None,
            inputs: Vec::new(),
            given_at: Vec::new(),
            records,
            plants: self.plants.clone(),
            moments: Some(Vec::new()),
            steps: Some(Vec::new()),
            heard: Vec::new(),
            glyphs: self.glyphs.clone(),
        };
        page.begin();
        page
    }

    /// Plays the recording to its end on its own clock, and returns each moment's time. The
    /// client must keep a record of its scripts (`fresh(true)`), whose spans give the steps'
    /// times.
    fn play_through(&mut self) -> Vec<(Name, u64)> {
        let mut moments = vec![(Name::Start, 0)];
        let mut prompts = 0;
        loop {
            self.still();
            for view in self.batch..self.released {
                moments.push((Name::View(view as u32 + 1), self.now));
                if matches!(self.views[view].view.status, Status::Deciding { .. }) {
                    prompts += 1;
                    moments.push((Name::Prompt(prompts), self.now));
                }
                self.step_moments(view, &mut moments);
            }
            if self.released >= self.views.len() {
                break;
            }
            let due = self.now + self.reply_delay().0;
            self.advance_to(due);
        }
        moments.push((Name::End, self.now));
        moments
    }

    /// Adds a moment for each step of the script of `view` (counted from 0) at the middle of the
    /// span the client played it over. The client is given one script with each view, in order,
    /// and keeps a record of each from the start, so the view's index is its script's.
    fn step_moments(&self, view: usize, moments: &mut Vec<(Name, u64)>) {
        let Some(record) = self.client.history().get(view) else {
            return;
        };
        let number = view as u32 + 1;
        for step in &record.steps {
            let Some((from, to)) = record.span(step) else {
                continue;
            };
            let name = Name::Step(number, step.beat as u32, step.index as u32);
            moments.push((name, from + (to - from) / 2));
        }
    }
}

/// How far apart two moments' names are: whether they are of different kinds, then how far
/// apart their numbers are, the view first.
fn moment_distance(a: Name, b: Name) -> (bool, u32, u32, u32) {
    match (a, b) {
        (Name::Step(v, e, s), Name::Step(w, f, t)) => {
            (false, v.abs_diff(w), e.abs_diff(f), s.abs_diff(t))
        }
        (Name::View(v), Name::View(w)) | (Name::Prompt(v), Name::Prompt(w)) => {
            (false, v.abs_diff(w), 0, 0)
        }
        _ => (
            std::mem::discriminant(&a) != std::mem::discriminant(&b),
            0,
            0,
            0,
        ),
    }
}

/// `time` moved by `offset`, in nanoseconds.
fn offset_from(time: u64, offset: Offset) -> u64 {
    if offset.before {
        time.saturating_sub(offset.length.0)
    } else {
        time + offset.length.0
    }
}
