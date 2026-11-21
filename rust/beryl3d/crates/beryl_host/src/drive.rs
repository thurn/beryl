//! Drive scripts (docs/beryl-3d.md, "Driving the running app"): a file of actions that the
//! host plays on the display's clock, through the same inputs a person's events make, so that
//! an agent can reproduce what a person does at the window without one. When the last action
//! has been given, the run ends.
//!
//! A script is one action a line, as `--do` and the test driver write them ("Times, moments,
//! locators and actions"), each optionally beginning `at <duration>`: the time since the host
//! started at which it is given. A line with no `at` is given as soon as the line before it
//! is done. `step <duration>` waits that long before the next line, `still` waits until
//! nothing on the table moves, and a drag takes eight moves 16 ms apart, as the test driver's
//! does; `hold` makes the same moves and does not let go. Blank lines and lines beginning `#`
//! are skipped.
//!
//! A locator is found in the frame at the tick its action is given, and the pointer goes to
//! the centre of the element's largest uncovered rectangle, as the test driver's does. A point
//! `<x>,<y>` in place of a locator is that point of the window.

use std::collections::VecDeque;
use std::path::Path;

use beryl_scene::Client;
use beryl_scene::Input;
use beryl_scene::agent::{locate, uncovered};
use beryl_scene::frame::Wake;
use beryl_scene::input::{
    Button, Key, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, TextChange,
};
use game_probe::driver::{Action, Duration, point as written_point};
use glam::Vec2;

/// How many moves a drag makes between its press and its release.
const DRAG_MOVES: u32 = 8;
/// The time between two moves of a drag, in nanoseconds.
const DRAG_STEP: u64 = 16_000_000;

/// One line of a script.
#[derive(Clone, Debug)]
struct Line {
    /// The line's number in the file, from 1, for errors.
    number: usize,
    /// When it is given, in nanoseconds since the host started, if it says.
    at: Option<u64>,
    action: Action,
}

/// A script being played.
#[derive(Debug)]
pub struct Drive {
    /// The script's name, for errors.
    name: String,
    lines: Vec<Line>,
    /// The next line to give.
    next: usize,
    /// The earliest time the next line may be given, in nanoseconds since the host started.
    cursor: u64,
    /// Inputs of an action already begun, each with when it is due: a drag's moves.
    pending: VecDeque<(u64, Input)>,
    /// Whether the script waits for the table to stop moving (`still`).
    waiting_still: bool,
    /// Why the script stopped early, if it did.
    error: Option<String>,
}

impl Drive {
    /// Reads the script at `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or a line is not an action.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Self::parse(&path.display().to_string(), &text)
    }

    /// Reads a script from `text`, naming it `name` in errors.
    ///
    /// # Errors
    ///
    /// When a line is not an action.
    pub fn parse(name: &str, text: &str) -> Result<Self, String> {
        let mut lines = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let fail = |why: String| format!("{name}:{number}: {why}");
            let (at, action) = match line.strip_prefix("at ") {
                Some(rest) => {
                    let (time, action) = rest
                        .trim_start()
                        .split_once(' ')
                        .ok_or_else(|| fail(format!("'{line}' needs an action after its time")))?;
                    let time: Duration = time.parse().map_err(|error| fail(format!("{error}")))?;
                    (Some(time.nanos()), action)
                }
                None => (None, line),
            };
            let action = action.parse().map_err(|error| fail(format!("{error}")))?;
            lines.push(Line { number, at, action });
        }
        Ok(Self {
            name: name.to_owned(),
            lines,
            next: 0,
            cursor: 0,
            pending: VecDeque::new(),
            waiting_still: false,
            error: None,
        })
    }

    /// Starts the script at `now`, in nanoseconds since the host started, instead of at the
    /// host's start: for a script handed over while the run is going.
    pub fn start_at(&mut self, now: u64) {
        self.cursor = now;
    }

    /// Whether every action has been given, or the script stopped on an error.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.error.is_some()
            || (self.next >= self.lines.len() && self.pending.is_empty() && !self.waiting_still)
    }

    /// Why the script stopped before its end, if it did: a locator that found nothing.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// When the script next wants a tick, given the client's own wake.
    #[must_use]
    pub fn wake(&self, client: Wake) -> Wake {
        if self.finished() {
            return Wake::Never;
        }
        if let Some((at, _)) = self.pending.front() {
            return Wake::At(*at);
        }
        if self.waiting_still {
            // While the table moves, the client's own wake brings the ticks that notice it stop.
            return if client == Wake::Never {
                Wake::Now
            } else {
                Wake::Never
            };
        }
        self.lines
            .get(self.next)
            .map_or(Wake::Never, |line| Wake::At(self.time_of(line)))
    }

    /// Puts in `inputs` every input due at `now`, in nanoseconds since the host started.
    /// `client` is asked for its frame at `clock` when an action names a locator.
    pub fn due(&mut self, now: u64, clock: u64, client: &mut Client, inputs: &mut Vec<Input>) {
        loop {
            while let Some(&(at, _)) = self.pending.front()
                && at <= now
            {
                if let Some((_, input)) = self.pending.pop_front() {
                    inputs.push(input);
                }
            }
            if !self.pending.is_empty() || self.error.is_some() {
                return;
            }
            if self.waiting_still {
                if client.wake() != Wake::Never {
                    return;
                }
                self.waiting_still = false;
                self.cursor = self.cursor.max(now);
            }
            let Some(line) = self.lines.get(self.next).cloned() else {
                return;
            };
            let at = self.time_of(&line);
            if at > now {
                return;
            }
            self.next += 1;
            self.cursor = at;
            if let Err(why) = self.give(&line.action, at, clock, client) {
                self.error = Some(format!("{}:{}: {why}", self.name, line.number));
                return;
            }
        }
    }

    /// When `line` is given: its own time, but never before the line before it is done.
    fn time_of(&self, line: &Line) -> u64 {
        line.at.map_or(self.cursor, |at| at.max(self.cursor))
    }

    /// Queues the inputs `action` makes from `at`, and moves the cursor past them.
    fn give(
        &mut self,
        action: &Action,
        at: u64,
        clock: u64,
        client: &mut Client,
    ) -> Result<(), String> {
        let mut point = |locator: &str| point(client, clock, locator);
        match action {
            Action::Press(locator) => {
                let point = point(locator)?;
                self.pointer(at, PointerPhase::Down, point, Button::Primary);
                self.pointer(at, PointerPhase::Up, point, Button::Primary);
            }
            Action::Menu(locator) => {
                let point = point(locator)?;
                self.pointer(at, PointerPhase::Down, point, Button::Secondary);
                self.pointer(at, PointerPhase::Up, point, Button::Secondary);
            }
            Action::Hover(locator) => {
                let point = point(locator)?;
                self.pointer(at, PointerPhase::Move, point, Button::None);
            }
            Action::Drag(from, to) | Action::Hold(from, to) => {
                let start = point(from)?;
                let end = point(to)?;
                self.pointer(at, PointerPhase::Down, start, Button::Primary);
                let mut time = at;
                for move_number in 1..=DRAG_MOVES {
                    time += DRAG_STEP;
                    let point = start.lerp(end, move_number as f32 / DRAG_MOVES as f32);
                    self.pointer(time, PointerPhase::Move, point, Button::Primary);
                }
                if matches!(action, Action::Drag(..)) {
                    self.pointer(time, PointerPhase::Up, end, Button::Primary);
                }
                self.cursor = time;
            }
            Action::Type(locator, text) => {
                let point = point(locator)?;
                self.pointer(at, PointerPhase::Down, point, Button::Primary);
                self.pointer(at, PointerPhase::Up, point, Button::Primary);
                self.pending
                    .push_back((at, Input::Text(TextChange::Insert(text.clone()))));
            }
            Action::Key(chord) => {
                let modifiers = chord.modifiers(client.mac());
                for phase in [KeyPhase::Down, KeyPhase::Up] {
                    let key = Key {
                        code: chord.code,
                        phase,
                        modifiers,
                    };
                    self.pending.push_back((at, Input::Key(key)));
                }
            }
            Action::Move(point) => self.pointer(at, PointerPhase::Move, *point, Button::None),
            Action::Wheel(locator, by) => {
                let point = point(locator)?;
                self.pending.push_back((
                    at,
                    Input::Pointer(Pointer {
                        wheel: *by,
                        ..mouse(PointerPhase::Wheel, point, Button::None)
                    }),
                ));
            }
            Action::Step(duration) => self.cursor = at + duration.nanos(),
            Action::Still => self.waiting_still = true,
        }
        Ok(())
    }

    fn pointer(&mut self, at: u64, phase: PointerPhase, position: Vec2, button: Button) {
        let pointer = mouse(phase, position, button);
        self.pending.push_back((at, Input::Pointer(pointer)));
    }
}

/// The mouse's event of `phase` at `position`, with no wheel turned and no modifier held.
fn mouse(phase: PointerPhase, position: Vec2, button: Button) -> Pointer {
    Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position,
        button,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    }
}

/// Where an action on `locator`'s element happens in the client's frame at `clock`: the centre
/// of its largest uncovered rectangle, or the point itself when `locator` writes one.
fn point(client: &mut Client, clock: u64, locator: &str) -> Result<Vec2, String> {
    if let Some(at) = written_point(locator) {
        return Ok(at);
    }
    client.prepare(clock);
    let frame = client.frame(clock);
    let index = locate(frame, locator).map_err(|error| error.to_string())?;
    let rect = uncovered(frame, index)
        .ok_or_else(|| format!("no part of '{locator}' can be pressed: it is covered"))?;
    Ok(Vec2::new(
        rect.x + rect.width / 2.0,
        rect.y + rect.height / 2.0,
    ))
}
