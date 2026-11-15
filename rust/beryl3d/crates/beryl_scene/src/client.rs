//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "Components" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

use std::collections::VecDeque;

use crate::command::{Request, SessionCommand};
use crate::frame::{self, Frame, FrameBuilder, Wake};
use beryl_session::GameUpdate;

use crate::input::{GlyphKey, Input};
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
    /// The latest view from the session.
    update: Option<Box<GameUpdate>>,
    window: Window,
    table: Table,
    wake: Wake,
    requests: VecDeque<Request>,
    commands: VecDeque<SessionCommand>,
    glyphs: VecDeque<GlyphKey>,
}

impl Client {
    #[must_use]
    pub fn new(config: Config) -> Self {
        Self {
            frame: Frame::with_capacity(config.elements, config.draws, config.name_bytes),
            inputs: Vec::new(),
            update: None,
            window: Window::default(),
            table: Table::default(),
            wake: Wake::Never,
            requests: VecDeque::new(),
            commands: VecDeque::new(),
            glyphs: VecDeque::new(),
        }
    }

    /// Takes a session update, a pointer or key event, a new surface or another input. An
    /// input has no time of its own: what it starts begins at the `now` of the next `frame`.
    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Update(update) => {
                self.table.apply(&update.view);
                self.update = Some(update);
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
                let view = self.update.as_ref().map(|update| &update.view);
                self.table.resize(self.window, view);
            }
            input => self.inputs.push(input),
        }
        self.wake = Wake::Now;
    }

    /// The frame at `now`, in nanoseconds since the client started.
    pub fn frame(&mut self, _now: u64) -> &Frame {
        self.inputs.clear();
        self.wake = Wake::Never;
        let mut builder = FrameBuilder::new(&mut self.frame);
        if let Some(update) = &self.update {
            self.table.write(&update.view, &mut builder);
        }
        builder.finish()
    }

    /// The table, for tests and tools that read the camera.
    #[must_use]
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// When the next frame is needed: as of the last call to `frame`, or `Now` once an input
    /// has arrived since, because any input ends the wait.
    #[must_use]
    pub fn wake(&self) -> Wake {
        self.wake
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
        self.glyphs.drain(..)
    }
}
