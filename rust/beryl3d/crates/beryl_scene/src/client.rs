//! The client core's two entry points, `Client::handle` and `Client::frame`, and the queues a
//! host drains after each frame (docs/beryl-3d.md, "Components" and "Hosts"). Neither entry
//! point reads a clock, starts a thread, takes a lock or touches a file, so the same calls give
//! the same frames.

use std::collections::VecDeque;

use beryl_session::prompt::{Click, ClickReply, ClickRequest, PromptView};
use beryl_session::protocol::{DecisionId, Status};
use beryl_session::script::Verb;

use crate::command::{Reply, Request, SessionCommand};
use crate::element::{Press, prompt_bar};
use crate::frame::{self, ElementId, Frame, FrameBuilder, Wake};
use crate::input::{AssetReady, Button, GlyphKey, Input, Pointer, PointerKind, PointerPhase};
use crate::layout::Layer;
use crate::play::Show;
use crate::play::record::{Plant, ScriptRecord};
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
    /// The flat layer: the prompt bar.
    layer: Layer,
    /// The open decision and its prompt's latest state, from the view or a click's reply.
    prompt: Option<(DecisionId, PromptView)>,
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
            prompt: None,
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
                self.prompt = match &update.view.status {
                    Status::Deciding { decision } => {
                        Some((decision.id, (*decision.prompt).clone()))
                    }
                    _ => None,
                };
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
                self.show_prompt();
            }
            Input::Reply(Reply::Click(Ok(reply))) => {
                match reply {
                    ClickReply::Open { prompt } => {
                        if let Some((_, open)) = &mut self.prompt {
                            *open = *prompt;
                        }
                    }
                    // The views that follow the answer bring the next prompt.
                    ClickReply::Answered => self.prompt = None,
                }
                self.show_prompt();
            }
            Input::Pointer(pointer) => self.point(pointer),
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
        let mut builder = FrameBuilder::new(&mut self.frame);
        self.show.frame(now, &mut builder);
        self.wake = self.show.wake(now);
        let (layout_nodes, text_runs) = self.layer.take_work();
        builder.layout_work(layout_nodes, text_runs);
        self.layer.write(&mut builder, self.show.table().camera());
        builder.finish();
        if let Some(point) = self.mouse
            && let Some(index) = crate::agent::hit(&self.frame, point)
        {
            self.frame.resolved[index as usize].state.0 |= frame::StateBits::HOVERED;
        }
        &self.frame
    }

    /// Rebuilds the prompt bar from the open prompt, or takes it away.
    fn show_prompt(&mut self) {
        let screen = self.prompt.as_ref().map(|(_, prompt)| prompt_bar(prompt));
        self.layer.show(screen);
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
        match pointer.phase {
            PointerPhase::Down if primary => {
                self.pressed = self.layer.press_at(pointer.position).map(|(id, ..)| id);
            }
            PointerPhase::Up if primary => {
                let pressed = self.pressed.take();
                if let Some((id, press, true)) = self.layer.press_at(pointer.position)
                    && pressed == Some(id)
                {
                    self.press(press);
                }
            }
            PointerPhase::Cancel | PointerPhase::Leave => self.pressed = None,
            _ => {}
        }
    }

    fn press(&mut self, press: Press) {
        let Some((decision, prompt)) = &self.prompt else {
            return;
        };
        match press {
            Press::Button(button) => {
                self.commands.push_back(SessionCommand::Click(ClickRequest {
                    decision: *decision,
                    revision: prompt.revision,
                    click: Click::Button { button },
                }));
            }
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
