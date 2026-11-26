//! The report's capture, region selection, description and save lifecycle.

mod view;

pub use view::compose;

use game_table::profiles::Profile;
use game_table::table::Window;
use glam::Vec2;
use kit_base::data::profiles::BugScreen;
use kit_base::tokens::size;
use kit_frame::frame::Wake;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::input::{Button, Insets, Pointer, PointerPhase, Rect, Size};
use kit_ui::widgets::FieldState;

pub const DESCRIPTION: &str = "description";

const CLICK: f32 = size::BUG_CLICK;
const SAVED: u64 = 900_000_000;
// The first frame is followed by the host's FreezeFrame request. The host reads
// back the second frame. Neither includes the report.
const CAPTURE_FRAMES: u8 = 2;

pub struct BugReport;

#[derive(Clone, Debug, PartialEq)]
pub enum BugStage {
    Capturing {
        drawn: u8,
    },
    Select {
        drag: Option<Rect>,
    },
    Describe {
        region: Rect,
        saving: bool,
        error: Option<String>,
    },
    Saved {
        region: Rect,
        close_at: Option<u64>,
    },
}

#[derive(Debug)]
pub struct State {
    pub stage: BugStage,
    pub screen_open: bool,
    ai: bool,
    description: String,
    frozen: Option<u64>,
    behind: u64,
    from: Option<Vec2>,
    whole: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            stage: BugStage::Capturing { drawn: 0 },
            screen_open: false,
            ai: false,
            description: String::new(),
            frozen: None,
            behind: 0,
            from: None,
            whole: false,
        }
    }
}

impl State {
    /// The table clock pauses while the report is open.
    #[must_use]
    pub fn table_time(&self, now: u64) -> u64 {
        self.frozen.unwrap_or(now.saturating_sub(self.behind))
    }

    #[must_use]
    pub fn frame_time(&self, table: u64) -> u64 {
        table + self.behind
    }

    #[must_use]
    pub fn wake(&self) -> Wake {
        match self.stage {
            // A save in flight is work that has not finished: a script's `still` must wait for
            // it, because the report cannot be opened again until "Saved" has closed.
            BugStage::Capturing { .. }
            | BugStage::Describe { saving: true, .. }
            | BugStage::Saved { close_at: None, .. } => Wake::Now,
            BugStage::Saved {
                close_at: Some(at), ..
            } => Wake::At(at),
            _ => Wake::Never,
        }
    }

    fn choose(&mut self, region: Rect, context: &mut Effects<'_>) {
        self.stage = BugStage::Describe {
            region,
            saving: false,
            error: None,
        };
        context.request(Request::FocusDescription);
    }

    fn drag(&mut self, pointer: &Pointer, window: Rect, context: &mut Effects<'_>) -> bool {
        let primary = pointer.button == Button::Primary;
        let at = pointer.position;
        match (pointer.phase, self.from) {
            (PointerPhase::Down, _) if primary => {
                self.from = Some(at);
                self.stage = BugStage::Select {
                    drag: Some(span(at, at)),
                };
            }
            (PointerPhase::Move, Some(from)) => {
                self.stage = BugStage::Select {
                    drag: Some(span(from, at)),
                };
            }
            (PointerPhase::Up, Some(from)) if primary => {
                self.from = None;
                let region = span(from, at);
                let click = region.width < CLICK || region.height < CLICK;
                self.choose(if click { window } else { region }, context);
            }
            (PointerPhase::Cancel | PointerPhase::Leave, Some(_)) => {
                self.from = None;
                self.stage = BugStage::Select { drag: None };
            }
            _ => return false,
        }
        true
    }

    fn tick(&mut self, now: u64, window: Rect, context: &mut Effects<'_>) -> bool {
        match self.stage {
            BugStage::Capturing { drawn } if drawn < CAPTURE_FRAMES => {
                self.stage = BugStage::Capturing { drawn: drawn + 1 };
            }
            BugStage::Capturing { .. } => {
                self.stage = BugStage::Select { drag: None };
                if self.whole {
                    self.choose(window, context);
                }
                return true;
            }
            BugStage::Saved {
                region,
                close_at: None,
            } => {
                self.stage = BugStage::Saved {
                    region,
                    close_at: Some(now + SAVED),
                };
            }
            BugStage::Saved {
                close_at: Some(at), ..
            } if now >= at => {
                context.request(Request::Close);
            }
            _ => {}
        }
        false
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Open,
    Close,
    Opened { now: u64, screen_open: bool },
    Closed(u64),
    Tick { now: u64, window: Rect },
    Drag { pointer: Pointer, window: Rect },
    Whole(Rect),
    Ai,
    Description(String),
    Save(Size),
    Saved(Result<(), String>),
    Export,
}

pub enum Request {
    Open,
    Close,
    FreezeFrame,
    FocusDescription,
    ClearFocus,
    Save {
        description: String,
        ai: bool,
        region: Rect,
        window: Size,
    },
    Export,
}

type Effects<'a> = Context<'a, std::convert::Infallible, Request>;

/// The resolved profile slots and dimensions used by the report's view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    metrics: Option<BugScreen>,
    hint: Option<Rect>,
    sheet: Option<Rect>,
    tools: Option<Rect>,
    safe_area: Insets,
    target: f32,
}

impl Layout {
    #[must_use]
    pub fn new(profile: Profile<'_>) -> Self {
        Self {
            metrics: profile.data().screen.map(|screen| screen.bug),
            hint: profile.slot("bug.hint").map(|slot| slot.rect),
            sheet: profile.slot("bug.sheet").map(|slot| slot.rect),
            tools: profile.slot("bug.tools").map(|slot| slot.rect),
            safe_area: profile.window().safe_area,
            target: profile.data().checks.target_points,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    pub window: Window,
    pub layout: Layout,
    pub description: FieldState,
}

impl Feature for BugReport {
    const SOURCE: &'static str = file!();

    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs;
    type ViewKey = Inputs;
    type Command = std::convert::Infallible;
    type Request = Request;

    fn update(state: &mut State, message: &Message, context: &mut Effects<'_>) -> bool {
        match message {
            Message::Open => context.request(Request::Open),
            Message::Close => context.request(Request::Close),
            Message::Opened { now, screen_open } => {
                *state = State {
                    screen_open: *screen_open,
                    frozen: Some(state.table_time(*now)),
                    behind: state.behind,
                    ..State::default()
                };
                context.request(Request::FreezeFrame);
                return true;
            }
            Message::Closed(now) => {
                if let Some(frozen) = state.frozen.take() {
                    state.behind = now.saturating_sub(frozen);
                }
            }
            Message::Tick { now, window } => return state.tick(*now, *window, context),
            Message::Drag { pointer, window } => return state.drag(pointer, *window, context),
            Message::Whole(window) => match state.stage {
                BugStage::Capturing { .. } => state.whole = true,
                BugStage::Select { .. } => {
                    state.choose(*window, context);
                    return true;
                }
                _ => {}
            },
            Message::Ai => {
                state.ai = !state.ai;
                return true;
            }
            Message::Description(text) => state.description.clone_from(text),
            Message::Save(window) => {
                let BugStage::Describe {
                    region,
                    saving: false,
                    ..
                } = state.stage
                else {
                    return false;
                };
                if state.description.trim().is_empty() {
                    return false;
                }
                state.stage = BugStage::Describe {
                    region,
                    saving: true,
                    error: None,
                };
                context.request(Request::Save {
                    description: state.description.clone(),
                    ai: state.ai,
                    region,
                    window: *window,
                });
                return true;
            }
            Message::Saved(result) => {
                let BugStage::Describe { region, .. } = state.stage else {
                    return false;
                };
                state.stage = match result {
                    Ok(()) => {
                        context.request(Request::ClearFocus);
                        BugStage::Saved {
                            region,
                            close_at: None,
                        }
                    }
                    Err(error) => BugStage::Describe {
                        region,
                        saving: false,
                        error: Some(error.clone()),
                    },
                };
                return true;
            }
            Message::Export => context.request(Request::Export),
        }
        false
    }

    fn view_key(_: &State, inputs: &Inputs) -> Inputs {
        inputs.clone()
    }

    fn view(state: &State, inputs: Inputs) -> Element {
        view::overlay(state, inputs)
    }
}

#[must_use]
pub fn whole_window(size: Size) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: size.width,
        height: size.height,
    }
}

fn span(a: Vec2, b: Vec2) -> Rect {
    let (low, high) = (a.min(b), a.max(b));
    Rect {
        x: low.x,
        y: low.y,
        width: high.x - low.x,
        height: high.y - low.y,
    }
}
