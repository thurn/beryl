//! The client's side of the bug report (`screens::bug_report`): opening it freezes the table,
//! the host takes the picture, a drag chooses the region, and Submit asks the host to save the
//! report, as the web interface's `BugReporter` (`ui/src/components/BugReport.tsx`) does.
//!
//! The table freezes by its clock: while the report is open, every part of the client that
//! plays the table is given the time it froze at, and once it closes the table's clock runs
//! that much behind the frame's, so what was playing goes on from where it stopped, as the
//! web's `thaw` lets the page play on. The flat layer keeps the frame's clock, so the report's
//! own panel fades in.

use glam::Vec2;

use super::{Client, Platform};
use crate::command::Request;
use crate::element::{BugPress, Chord};
use crate::frame::Wake;
use crate::input::{Button, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerPhase, Rect};
use crate::screens::{BugForm, BugStage, CLICK, DESCRIPTION, Sheet, span};
use crate::widgets::FieldState;

/// How long "Saved" shows before the report closes, in nanoseconds (`SAVED_MS`).
const SAVED: u64 = 900_000_000;

/// How many frames the report draws nothing of itself after it froze the table: the frame
/// drawn after Cmd+B, after which the host sees `Request::FreezeFrame`, and the next one,
/// which the host reads back as the picture.
const CAPTURE_FRAMES: u8 = 2;

/// The bug report's state beside its `BugForm`, and the table's clock.
#[derive(Debug, Default)]
pub(super) struct Bug {
    pub(super) form: BugForm,
    /// The table's time when the report froze it, while it is open.
    frozen: Option<u64>,
    /// How far the table's clock runs behind the frame's: the time it has spent frozen.
    behind: u64,
    /// Where the primary button went down to drag out a region.
    from: Option<Vec2>,
    /// Whether the whole window was asked for while the picture was being taken.
    whole: bool,
    /// Whether the screen was drawn when the report froze the table: the flat layer's screen
    /// is drawn only once the table has settled (`Client::open`), and a frozen table never
    /// settles.
    screen_open: bool,
}

impl Bug {
    /// The table's time at the frame time `now`.
    pub(super) fn table(&self, now: u64) -> u64 {
        self.frozen.unwrap_or(now.saturating_sub(self.behind))
    }

    /// The frame time at the table's time `table`.
    pub(super) fn frame_time(&self, table: u64) -> u64 {
        table + self.behind
    }
}

impl Client {
    /// Whether the bug report is open.
    pub(super) fn bug_open(&self) -> bool {
        self.sheet == Some(Sheet::BugReport)
    }

    /// Whether the table's floats (life totals, pile counts, notices and the gear) show: with
    /// no sheet over the screen, or while the bug report takes its picture, which shows them.
    /// Floats are drawn over the screen, so once the picture is taken they would be drawn over
    /// the report.
    pub(super) fn table_shown(&self) -> bool {
        self.sheet.is_none()
            || (self.bug_open() && matches!(self.bug.form.stage, BugStage::Capturing { .. }))
    }

    /// Whether the prompt's marks and the screen show as the table stands: as they stood when
    /// the report froze it, while it is open.
    pub(super) fn table_open(&self) -> bool {
        if self.bug_open() {
            self.bug.screen_open
        } else {
            self.open
        }
    }

    /// Whether the screen is left out under the report, because it was not drawn when the
    /// report froze the table. While the report is open the flat layer is always drawn, so
    /// that the report shows.
    pub(super) fn bug_hides_screen(&self) -> bool {
        self.bug_open() && !self.bug.screen_open
    }

    /// Opens `sheet` over the screen, or closes the open one. Opening the bug report freezes
    /// the table and asks the host for its picture; closing it lets the table play on. Cmd+B
    /// while the report is open does nothing.
    pub(super) fn set_sheet(&mut self, sheet: Option<Sheet>) {
        let was = self.bug_open();
        let opening = sheet == Some(Sheet::BugReport);
        if was && opening {
            return;
        }
        if was {
            if let Some(frozen) = self.bug.frozen.take() {
                self.bug.behind = self.now.saturating_sub(frozen);
            }
            self.layer.focus(None);
        }
        self.sheet = sheet;
        // Closing or changing the sheet puts the confirmation back.
        self.concede = crate::screens::Concede::Asks;
        if opening {
            self.bug.frozen = Some(self.bug.table(self.now));
            self.bug.screen_open = self.open;
            self.bug.form = BugForm::new();
            self.bug.from = None;
            self.bug.whole = false;
            self.pressed = None;
            self.layer.focus(None);
            self.fields.set(DESCRIPTION, FieldState::default());
            self.requests.push_back(Request::FreezeFrame);
        }
        self.show_screen();
    }

    /// Carries out a press of one of the report's controls.
    pub(super) fn bug_press(&mut self, press: BugPress) {
        match press {
            BugPress::Whole => match self.bug.form.stage {
                // As the web's `wantWhole`: the window is taken once the picture is in.
                BugStage::Capturing { .. } => self.bug.whole = true,
                BugStage::Select { .. } => self.choose(self.whole_window()),
                _ => {}
            },
            BugPress::Ai => {
                self.bug.form.ai = !self.bug.form.ai;
                self.show_screen();
            }
            BugPress::Save => self.save_bug(),
        }
    }

    /// The host's answer to `Request::SaveBug`: "Saved", or the error under the description.
    pub(super) fn bug_saved(&mut self, result: Result<(), String>) {
        let BugStage::Describe { region, .. } = self.bug.form.stage else {
            return;
        };
        self.bug.form.stage = match result {
            Ok(()) => {
                self.layer.focus(None);
                BugStage::Saved {
                    region,
                    close_at: None,
                }
            }
            Err(error) => BugStage::Describe {
                region,
                saving: false,
                error: Some(error),
            },
        };
        self.show_screen();
    }

    /// What the report does at the start of the frame at `now`: shows the region chooser once
    /// the picture has been taken, and closes once "Saved" has shown long enough. Returns the
    /// table's time.
    pub(super) fn bug_frame(&mut self, now: u64) -> u64 {
        if self.bug_open() {
            match self.bug.form.stage {
                BugStage::Capturing { drawn } if drawn < CAPTURE_FRAMES => {
                    self.bug.form.stage = BugStage::Capturing { drawn: drawn + 1 };
                }
                BugStage::Capturing { .. } => {
                    self.bug.form.stage = BugStage::Select { drag: None };
                    if self.bug.whole {
                        self.choose(self.whole_window());
                    } else {
                        self.show_screen();
                    }
                }
                BugStage::Saved {
                    region,
                    close_at: None,
                } => {
                    self.bug.form.stage = BugStage::Saved {
                        region,
                        close_at: Some(now + SAVED),
                    };
                }
                BugStage::Saved {
                    close_at: Some(at), ..
                } if now >= at => self.set_sheet(None),
                _ => {}
            }
        }
        self.bug.table(now)
    }

    /// When the next frame is needed for the table, whose time is `table`, and for the report:
    /// at once while the picture is being taken, never for a frozen table, and when "Saved"
    /// closes.
    pub(super) fn bug_wake(&self, table: u64) -> Wake {
        if !self.bug_open() {
            return match self.show.wake(table) {
                Wake::At(at) => Wake::At(self.bug.frame_time(at)),
                wake => wake,
            };
        }
        match self.bug.form.stage {
            BugStage::Capturing { .. } | BugStage::Saved { close_at: None, .. } => Wake::Now,
            BugStage::Saved {
                close_at: Some(at), ..
            } => Wake::At(at),
            _ => Wake::Never,
        }
    }

    /// A pointer event while the report is open: nothing reaches the table. While a region is
    /// being chosen, a drag outside the tools draws it, and a release chooses it (a click, the
    /// whole window); otherwise the pointer presses the report's controls.
    pub(super) fn bug_point(&mut self, pointer: &Pointer) {
        match self.bug.form.stage {
            BugStage::Capturing { .. } => return,
            BugStage::Select { .. } => {
                if !self.drag_region(pointer) {
                    self.press_flat(pointer, false);
                }
            }
            _ => self.press_flat(pointer, false),
        }
        self.hover_flat(pointer);
    }

    /// Draws or chooses the region `pointer` drags out, unless it went down on a tool.
    /// Returns whether it did.
    fn drag_region(&mut self, pointer: &Pointer) -> bool {
        let primary = pointer.button == Button::Primary;
        let at = pointer.position;
        match (pointer.phase, self.bug.from) {
            (PointerPhase::Down, _) if primary => {
                if self.layer.press_at(at, self.open).is_some() {
                    return false;
                }
                self.bug.from = Some(at);
                self.bug.form.stage = BugStage::Select {
                    drag: Some(span(at, at)),
                };
            }
            (PointerPhase::Move, Some(from)) => {
                self.bug.form.stage = BugStage::Select {
                    drag: Some(span(from, at)),
                };
            }
            (PointerPhase::Up, Some(from)) if primary => {
                self.bug.from = None;
                let region = span(from, at);
                let click = region.width < CLICK || region.height < CLICK;
                self.choose(if click { self.whole_window() } else { region });
                return true;
            }
            (PointerPhase::Cancel | PointerPhase::Leave, Some(_)) => {
                self.bug.from = None;
                self.bug.form.stage = BugStage::Select { drag: None };
            }
            _ => return false,
        }
        self.show_screen();
        true
    }

    /// A key while the report is open. Escape closes it. While the picture is taken or a region
    /// chosen, Space or Return takes the whole window and every other key is swallowed. While
    /// the bug is described, Cmd+Return submits, Tab and Return move the focus and press as
    /// usual, the description field takes what it types, and every other key is swallowed, so
    /// that none reaches the screen under the report. Returns whether the key means nothing
    /// else.
    pub(super) fn bug_key(&mut self, key: Key) -> bool {
        if !self.bug_open() {
            return false;
        }
        let first = key.phase == KeyPhase::Down;
        if key.code == KeyCode::Escape {
            if first {
                self.set_sheet(None);
            }
            return true;
        }
        match self.bug.form.stage {
            BugStage::Capturing { .. } | BugStage::Select { .. } => {
                let plain = key.modifiers == Modifiers::default();
                if first && plain && matches!(key.code, KeyCode::Space | KeyCode::Enter) {
                    self.bug_press(BugPress::Whole);
                }
                true
            }
            BugStage::Describe { .. } => {
                let mac = self.platform == Platform::Mac;
                if Chord::parse("Cmd+Return", mac) == Some(Chord::of(&key)) {
                    if first {
                        self.bug_press(BugPress::Save);
                    }
                    return true;
                }
                if self.layer.edited_id().is_some() && key.code != KeyCode::Tab {
                    return false;
                }
                match key.code {
                    KeyCode::Tab => self.layer.move_focus(!key.modifiers.shift),
                    KeyCode::Enter if first => {
                        if let Some(press) = self.layer.focused_press() {
                            self.press(&press);
                        }
                    }
                    _ => {}
                }
                true
            }
            BugStage::Saved { .. } => true,
        }
    }

    /// Goes on to describe `region`, with the description field focused.
    fn choose(&mut self, region: Rect) {
        self.bug.form.stage = BugStage::Describe {
            region,
            saving: false,
            error: None,
        };
        self.show_screen();
        self.layer.focus_field(DESCRIPTION);
    }

    /// Asks the host to save the report with the description typed, unless it is empty or
    /// already being saved.
    fn save_bug(&mut self) {
        let BugStage::Describe {
            region,
            saving: false,
            ..
        } = self.bug.form.stage
        else {
            return;
        };
        let description = self.field(DESCRIPTION).text;
        if description.trim().is_empty() {
            return;
        }
        self.bug.form.stage = BugStage::Describe {
            region,
            saving: true,
            error: None,
        };
        self.requests.push_back(Request::SaveBug {
            description,
            ai: self.bug.form.ai,
            region,
            window: self.window.size,
        });
        self.show_screen();
    }

    /// The whole window, in logical points.
    fn whole_window(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: self.window.size.width,
            height: self.window.size.height,
        }
    }
}
