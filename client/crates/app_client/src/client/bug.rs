//! Connects the report's messages and paused clock to the table and input routing.

use super::features::BUG_REPORT;
use super::{Client, input::ScenePoint};
use app_screens::screens::Sheet;
use app_screens::screens::bug_report::{BugStage, Message, whole_window};
use game_hud::hud::inspector;
use kit_frame::frame::Wake;
use kit_scene::input::KeyResponse;
use kit_ui::input::{Button, Key, KeyCode, KeyPhase, Modifiers, PointerPhase};

impl Client {
    /// Whether the bug report is open.
    pub(super) fn bug_open(&self) -> bool {
        self.features.sheet == Some(Sheet::BugReport)
    }

    /// Whether the table's floats (life totals, pile counts, notices and the gear) show.
    /// The report keeps drawing the frozen table, and its modal overlay paints over the floats.
    pub(super) fn table_shown(&self) -> bool {
        self.features.sheet.is_none() || self.bug_open()
    }

    /// Whether the prompt's marks and the screen show as the table stands: as they stood when
    /// the report froze it, while it is open.
    pub(super) fn table_open(&self) -> bool {
        if self.bug_open() {
            self.features.bug().screen_open
        } else {
            self.features.layer_open
        }
    }

    /// Whether the screen is left out under the report, because it was not drawn when the
    /// report froze the table. While the report is open the flat layer is always drawn, so
    /// that the report shows.
    pub(super) fn bug_hides_screen(&self) -> bool {
        self.bug_open() && !self.features.bug().screen_open
    }

    /// Opens `sheet` over the screen, or closes the open one. Opening the bug report freezes
    /// the table and asks the host for its picture; closing it lets the table play on. Cmd+B
    /// while the report is open does nothing.
    pub(super) fn set_sheet(&mut self, sheet: Option<Sheet>) {
        if self.change_sheet(sheet) {
            self.show_screen();
        }
    }

    /// Changes the route before the caller composes the new screen.
    pub(super) fn change_sheet(&mut self, sheet: Option<Sheet>) -> bool {
        let was = self.bug_open();
        let opening = sheet == Some(Sheet::BugReport);
        if was && opening {
            return false;
        }
        if was {
            self.features.inspect(&inspector::Message::Release);
            self.update_bug(&Message::Closed(self.now));
            self.layer.focus(None);
        }
        self.features.sheet = sheet;
        // Closing or changing the sheet resets the settings confirmation.
        self.features.reset_sheet();
        if opening {
            let hovered = self.hovered_slot();
            self.features.inspect(&inspector::Message::Hold(hovered));
            self.layer.focus(None);
            self.update_bug(&Message::Opened {
                now: self.now,
                screen_open: self.features.layer_open,
            });
        }
        true
    }

    /// Advances report capture or dismissal and returns the paused table clock.
    pub(super) fn bug_frame(&mut self, now: u64) -> u64 {
        if self.bug_open()
            && self.update_bug(&Message::Tick {
                now,
                window: whole_window(self.window.size),
            })
        {
            self.show_screen();
        }
        self.features.bug().table_time(now)
    }

    /// When the next frame is needed for the table, whose time is `table`, and for the report:
    /// at once while the picture is being taken, never for a frozen table, and when "Saved"
    /// closes.
    pub(super) fn bug_wake(&self, table: u64) -> Wake {
        if !self.bug_open() {
            return match self.show.wake(table) {
                Wake::At(at) => Wake::At(self.features.bug().frame_time(at)),
                wake => wake,
            };
        }
        self.features.bug().wake()
    }

    /// When the next thing that waits for a time acts, with nothing animating: the bug report's
    /// own (`bug_wake`), a tooltip waiting for the pointer to rest, a finger held still or
    /// waiting to pick a card up, and the word a refused click put under a permanent going.
    pub(super) fn timed_wake(&self, table: u64) -> Wake {
        let wake = self.bug_wake(table).or_at(self.layer.tip_due());
        let flip = if self.bug_open() {
            None
        } else {
            self.features
                .opening
                .spin_due()
                .map(|at| self.features.bug().frame_time(at))
        };
        wake.or_at(self.gesture_due())
            .or_at(self.features.inspector().notice_until())
            .or_at(flip)
    }

    /// A pointer event while the report is open: nothing reaches the table. While a region is
    /// being chosen, a drag outside the tools draws it, and a release chooses it (a click, the
    /// whole window); otherwise the pointer presses the report's controls.
    pub(super) fn bug_point(&mut self, point: &ScenePoint) {
        match self.features.bug().stage {
            BugStage::Capturing { .. } => return,
            BugStage::Select { .. } => {
                if !self.drag_region(point) {
                    self.press_flat(point);
                }
            }
            _ => {
                if !self.scroll_pointer(point) {
                    self.press_flat(point);
                }
            }
        }
        self.hover_flat(point);
    }

    /// Draws or chooses the region `pointer` drags out, unless it went down on a tool.
    /// Returns whether it did.
    fn drag_region(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        if pointer.phase == PointerPhase::Down
            && pointer.button == Button::Primary
            && point
                .flat
                .as_ref()
                .is_some_and(|target| target.press.is_some())
        {
            return false;
        }
        if !self.update_bug(&Message::Drag {
            pointer: *pointer,
            window: whole_window(self.window.size),
        }) {
            return false;
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
        match self.features.bug().stage {
            BugStage::Capturing { .. } | BugStage::Select { .. } => {
                let plain = key.modifiers == Modifiers::default();
                if first
                    && plain
                    && matches!(key.code, KeyCode::Space | KeyCode::Enter)
                    && self.update_bug(&Message::Whole(whole_window(self.window.size)))
                {
                    self.show_screen();
                }
                true
            }
            BugStage::Describe { .. } => {
                // The report's own keys (Cmd+Return saves it) come first.
                if let Some(response) = self.feature_shortcut(BUG_REPORT, key) {
                    if let KeyResponse::Action(action) = response {
                        self.dispatch(&action);
                    }
                    return true;
                }
                if self.layer.edited_id().is_some() && key.code != KeyCode::Tab {
                    self.field_key(key);
                    return true;
                }
                match key.code {
                    KeyCode::Tab => self
                        .layer
                        .move_focus(!key.modifiers.shift, self.features.layer_open),
                    KeyCode::Enter if first => {
                        if let Some(press) = self.layer.focused_press(self.features.layer_open) {
                            self.dispatch(&press);
                        }
                    }
                    _ => {
                        self.scroll_key(key);
                    }
                }
                true
            }
            BugStage::Saved { .. } => true,
        }
    }
}
