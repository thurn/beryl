//! How the client takes pointer and key input (docs/beryl-3d.md, "Hosts"): a press on the flat
//! layer or a card, a drag that scrolls or sets a slider, and the keys of shortcuts, focus and
//! the bug report.

use super::{Client, Platform};
use crate::agent::hit;
use crate::element::{Chord, Press};
use crate::frame::Role;
use crate::input::{Button, Input, Key, KeyCode, KeyPhase, Pointer, PointerKind, PointerPhase};
use crate::screens::{Screen, Sheet};
use crate::widgets::{STEP, level_at};

impl Client {
    /// A press is the primary button or a touch going down and coming up on the same enabled
    /// element. It queues the command the element's `Press` means. A press on a card of the
    /// table that no element of the flat layer covers goes to the table's pointer, which the
    /// next `frame` asks what it meant. A press on a slider, and a drag that begins on one,
    /// sets its level instead (`slide`). After each, the flat layer is told which element a
    /// press under the pointer would reach and which one is held down, which it draws so.
    pub(super) fn point(&mut self, pointer: Pointer) {
        self.show.pointer_mut().handle(&pointer);
        if self.scroll_layer(&pointer) {
            return;
        }
        if !self.slide(&pointer) {
            self.press_layer(&pointer);
        }
        let gone = matches!(pointer.phase, PointerPhase::Cancel | PointerPhase::Leave)
            || (pointer.kind == PointerKind::Touch && pointer.phase == PointerPhase::Up);
        let hovered = (!gone)
            .then(|| self.layer.press_at(pointer.position, self.open))
            .flatten()
            .map(|(id, ..)| id);
        (self.layer).point_at(hovered, self.pressed.or(self.slider));
    }

    /// Sets the level of the slider the primary pointer goes down on, and of the one it drags,
    /// to the level under the pointer, until the press ends; the slider takes the focus. Returns
    /// whether the pointer is on a slider, and so means nothing else.
    fn slide(&mut self, pointer: &Pointer) -> bool {
        let primary = pointer.button == Button::Primary;
        let at = match (pointer.phase, self.slider) {
            (PointerPhase::Down, _) if primary => {
                let Some((id, press, rect)) = self.layer.slider_at(pointer.position, self.open)
                else {
                    return false;
                };
                self.slider = Some(id);
                self.layer.focus(Some(id));
                (press, rect)
            }
            (PointerPhase::Move, Some(id)) => match self.layer.slider(id) {
                Some(at) => at,
                None => return true,
            },
            (PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave, Some(_)) => {
                self.slider = None;
                return true;
            }
            _ => return false,
        };
        let (press, rect) = at;
        if let Press::Setting(setting) = press
            && let Some(level) = setting.level()
        {
            let to = level_at(pointer.position.x, rect.x, rect.width);
            if to != level {
                self.press(&Press::Setting(setting.at_level(to)));
            }
        }
        true
    }

    /// A press on the flat layer or a card of the table, as `point` describes it.
    fn press_layer(&mut self, pointer: &Pointer) {
        let scroll = (self.row_gesture).handle(pointer, &self.frame, self.show.table());
        if let Some((row, first)) = scroll
            && self.show.scroll_row(row, first)
        {
            (self.row_counts).prepare(&mut self.layer, self.show.coming_rows());
        }
        let primary = pointer.button == Button::Primary;
        let open = self.open;
        match pointer.phase {
            PointerPhase::Down if primary => {
                self.pressed = (self.layer.press_at(pointer.position, open)).map(|(id, ..)| id);
                if let Some(id) = self.layer.focusable_at(pointer.position, open) {
                    self.layer.focus(Some(id));
                }
                if self.pressed.is_none()
                    && let Some(index) = hit(&self.frame, pointer.position)
                    && let element = &self.frame.resolved[index as usize]
                    && element.role == Role::Card
                {
                    let slot = element.id.0 as usize;
                    let holdable = self.holdable(slot, element.place);
                    (self.show.pointer_mut()).press(slot, pointer.position, holdable);
                }
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

    /// Scrolls the flat layer's element under `pointer` by the wheel, or by a touch dragged
    /// over it, which then scrolls it until the finger lifts. Returns whether the pointer
    /// scrolled the flat layer, and so means nothing else.
    fn scroll_layer(&mut self, pointer: &Pointer) -> bool {
        let open = self.open;
        match pointer.phase {
            PointerPhase::Wheel => {
                let Some(id) = self.layer.scroller_at(pointer.position, open) else {
                    return false;
                };
                // A wheel turned down scrolls toward the end, as in a browser.
                self.layer.scroll(id, -pointer.wheel);
                true
            }
            PointerPhase::Down if pointer.kind == PointerKind::Touch => {
                self.scroll_drag = (self.layer.scroller_at(pointer.position, open))
                    .map(|id| (pointer.id, id, pointer.position));
                false
            }
            PointerPhase::Move => match &mut self.scroll_drag {
                Some((finger, id, last)) if *finger == pointer.id => {
                    // The content follows the finger.
                    let delta = *last - pointer.position;
                    *last = pointer.position;
                    let id = *id;
                    self.layer.scroll(id, delta);
                    self.pressed = None;
                    true
                }
                _ => false,
            },
            PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave => {
                if self
                    .scroll_drag
                    .is_some_and(|(finger, ..)| finger == pointer.id)
                {
                    self.scroll_drag = None;
                }
                false
            }
            PointerPhase::Down => false,
        }
    }

    /// A key goes to the screen's shortcuts first, then moves the focus (Tab), presses the
    /// focused element (Return) or closes the sheet or menu on top (Escape), and otherwise
    /// opens the bug report on its chord: Cmd+B, or Cmd+Shift+B while the manual table is
    /// shown, where Cmd+B is the table's own (`isBugReportShortcut` in
    /// `ui/src/components/bugReportKeys.ts`). A key that means none of these waits for the next
    /// frame, as before.
    pub(super) fn key(&mut self, key: Key) {
        if key.phase == KeyPhase::Up {
            return;
        }
        let first = key.phase == KeyPhase::Down;
        let arrow = match key.code {
            KeyCode::ArrowLeft => Some(false),
            KeyCode::ArrowRight => Some(true),
            _ => None,
        };
        let bug_report = if self.screen == Screen::Table {
            "Cmd+Shift+B"
        } else {
            "Cmd+B"
        };
        let press = if let Some(press) = self.layer.shortcut(&key, self.platform == Platform::Mac) {
            Some(press)
        } else if key.code == KeyCode::Tab {
            self.layer.move_focus(!key.modifiers.shift);
            None
        } else if let Some(up) = arrow
            && let Some(press) = self.nudge(up)
        {
            Some(press)
        } else if first && key.code == KeyCode::Enter {
            self.layer.focused_press()
        } else if first && key.code == KeyCode::Escape {
            self.layer.close_press()
        } else if first
            && Chord::parse(bug_report, self.platform == Platform::Mac) == Some(Chord::of(&key))
        {
            Some(Press::Sheet(Some(Sheet::BugReport)))
        } else {
            self.inputs.push(Input::Key(key));
            None
        };
        if let Some(press) = press {
            self.press(&press);
        }
    }

    /// The press that moves the focused slider's level up a step, or down one if not `up`,
    /// as the left and right keys move a focused range input; `None` with no slider focused
    /// or at the end it would move past.
    fn nudge(&self, up: bool) -> Option<Press> {
        let Press::Setting(setting) = self.layer.focused_slider()? else {
            return None;
        };
        let level = setting.level()?;
        let to = if up {
            level.saturating_add(STEP).min(100)
        } else {
            level.saturating_sub(STEP)
        };
        (to != level).then_some(Press::Setting(setting.at_level(to)))
    }
}
