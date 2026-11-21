//! How the client takes pointer and key input (docs/beryl-3d.md, "Hosts"): a press on the flat
//! layer or a card, a drag that scrolls or sets a slider, and the keys of shortcuts, focus and
//! the bug report.

use beryl_protocol::prompt::{Click, ShapeBody};
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::gesture::{Gesture, Release};
use super::{Client, Dismiss, Input, Platform, Press};
use crate::agent::hit;
use crate::pointer::Hold;
use crate::pointer::rows::scroller_of;
use crate::prompts;
use crate::screens::{Screen, ScreenPress, Sheet};
use crate::settings::Setting;
use crate::table::{self, Scroller};
use kit_frame::frame::{ElementId, Place, Role};
use kit_ui::element::{Action, Chord};
use kit_ui::input::{Button, Key, KeyCode, KeyPhase, Pointer, PointerKind, PointerPhase};
use kit_ui::layout::{Edit, Navigation};
use kit_ui::widgets::{STEP, level_at};

impl Client {
    /// A press is the primary button or a touch going down and coming up on the same enabled
    /// element. It queues the command the element's `Press` means. A press on a card of the
    /// table that no element of the flat layer covers goes to the table's pointer, which the
    /// next `frame` asks what it meant. A press on a slider, and a drag that begins on one,
    /// sets its level instead (`slide`). After each, the flat layer is told which element a
    /// press under the pointer would reach and which one is held down, which it draws so.
    ///
    /// What the primary pointer is doing since it went down is one `Gesture`. A press going
    /// down ends the one under way before a branch claims the new one,
    /// and the release, Cancel or Leave ends it once every branch has seen that phase.
    ///
    /// A finger reaches these only if it is the active one (`hear_touch`), and a double tap
    /// opens the bug report once the game has heard the finger lift.
    pub(super) fn point(&mut self, pointer: Pointer) {
        let heard = (pointer.kind == PointerKind::Touch).then(|| self.hear_touch(&pointer));
        if heard.is_none_or(|heard| heard.game) {
            if pointer.phase == PointerPhase::Down && pointer.button == Button::Primary {
                self.end_gesture();
            }
            self.claim(&pointer);
            if ends_gesture(&pointer) {
                self.end_gesture();
            }
        }
        if heard.is_some_and(|heard| heard.report) && !self.bug_open() {
            self.dispatch_press(&Press::Sheet(Some(Sheet::BugReport)));
        }
    }

    /// Gives `pointer` to the first branch that claims it, as `point` describes.
    fn claim(&mut self, pointer: &Pointer) {
        let pointer = *pointer;
        if self.bug_open() {
            self.bug_point(&pointer);
            return;
        }
        self.show.pointer_mut().handle(&pointer);
        if pointer.phase == PointerPhase::Down {
            self.manual.shift = pointer.modifiers.shift;
        }
        // The mouse is followed whatever is asked, so that an arrow to the pointer that a
        // prompt opens under it starts there. A touch has no pointer to follow.
        self.owned.mouse = match (pointer.kind, pointer.phase) {
            (PointerKind::Mouse, PointerPhase::Leave | PointerPhase::Cancel) => None,
            (PointerKind::Mouse, _) => Some(pointer.position),
            _ => self.owned.mouse,
        };
        self.carry(self.show.pointer().held());
        // A press outside an open context menu closes it and means nothing else.
        if pointer.phase == PointerPhase::Down
            && pointer.button == Button::Primary
            && self.layer.outside_menu(pointer.position, self.open)
        {
            self.dispatch_press(&Press::Dismiss(Dismiss::Menu));
            return;
        }
        // A frozen table takes no gesture of its own; the flat layer's buttons still work.
        let frozen = self.table_frozen();
        if (!frozen && self.table_pointer(&pointer))
            || self.scroll_layer(&pointer)
            || self.open_table_menu(&pointer)
            || self.carry_item(&pointer)
        {
            return;
        }
        if !self.slide(&pointer) {
            self.press_layer(&pointer);
        }
        let hovered = self.hover_flat(&pointer);
        // The pointer on a payment's Auto Pay, or a touch held on it, marks what it would tap.
        let suggesting = if let (Some((_, prompt)), Some((_, action, true))) =
            (&self.prompt, &hovered)
            && let Some(Click::Button { button }) = action.downcast_ref::<Click>()
        {
            prompts::shows(prompt, *button)
        } else {
            false
        };
        self.suggest(suggesting);
    }

    /// Tells the flat layer which element a press under `pointer` would reach, which it draws
    /// hovered, and which one is held down, and returns the first with its press.
    pub(super) fn hover_flat(&mut self, pointer: &Pointer) -> Option<(ElementId, Action, bool)> {
        let gone = matches!(pointer.phase, PointerPhase::Cancel | PointerPhase::Leave)
            || (pointer.kind == PointerKind::Touch && pointer.phase == PointerPhase::Up);
        let hovered = (!gone)
            .then(|| self.layer.press_at(pointer.position, self.open))
            .flatten();
        let id = hovered.as_ref().map(|(id, ..)| *id);
        let held = (!ends_gesture(pointer))
            .then(|| self.gesture.held())
            .flatten();
        (self.layer).point_at(id, held);
        let at = (!gone).then_some(pointer.position);
        self.layer.hover(at, self.open);
        hovered
    }

    /// Sets the level of the slider the primary pointer goes down on, and of the one it drags,
    /// to the level under the pointer, until the press ends; the slider takes the focus. Returns
    /// whether the pointer is on a slider, and so means nothing else.
    fn slide(&mut self, pointer: &Pointer) -> bool {
        let primary = pointer.button == Button::Primary;
        let at = match (pointer.phase, self.gesture) {
            (PointerPhase::Down, _) if primary => {
                let Some((id, press, rect)) = self.layer.slider_at(pointer.position, self.open)
                else {
                    return false;
                };
                self.gesture = Gesture::Slider(id);
                self.layer.focus(Some(id));
                (press, rect)
            }
            (PointerPhase::Move, Gesture::Slider(id)) => match self.layer.slider(id) {
                Some(at) => at,
                None => return true,
            },
            (PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave, Gesture::Slider(_)) => {
                return true;
            }
            _ => return false,
        };
        let (press, rect) = at;
        if let Some(setting) = press.downcast_ref::<Setting>()
            && let Some(level) = setting.level()
        {
            let to = level_at(pointer.position.x, rect.x, rect.width);
            if to != level {
                self.dispatch_press(&Press::Setting(setting.at_level(to)));
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
        self.press_flat(pointer, true);
    }

    /// A press on the flat layer, or with `table` on a card of the table that no element of
    /// the flat layer covers, as `point` describes it.
    pub(super) fn press_flat(&mut self, pointer: &Pointer, table: bool) {
        let primary = pointer.button == Button::Primary;
        let open = self.open;
        match pointer.phase {
            PointerPhase::Down if primary => {
                let pressed = (self.layer.press_at(pointer.position, open)).map(|(id, ..)| id);
                self.gesture.set_release(pressed.map(Release::Element));
                if let Some(id) = self.layer.focusable_at(pointer.position, open) {
                    self.layer.focus(Some(id));
                    if self.layer.edited_id() == Some(id) {
                        self.press_field(pointer);
                    }
                }
                if table
                    && pressed.is_none()
                    && let Some(index) = hit(&self.frame, pointer.position)
                    && let element = &self.frame.resolved[index as usize]
                    && matches!(element.role, Role::Card | Role::Pile)
                {
                    // A press on a pile opens it, and is no press on the card on top. A card of a
                    // pile that takes presses itself, as one held up in a prompt's tray does,
                    // is pressed as a card.
                    let opens = element.role == Role::Pile || !element.interactive;
                    let pile = (self.pile_at(element.place))
                        .or_else(|| self.table_library_at(element.place));
                    if let Some(pile) = pile.filter(|_| opens) {
                        self.gesture.set_release(Some(Release::Pile(pile)));
                        return;
                    }
                    if element.role == Role::Card {
                        let slot = element.id.0 as usize;
                        // A drag sideways on a hand that scrolls scrolls it.
                        let scrolls = scroller_of(element.place) == Some(Scroller::Hand)
                            && self.show.table().row_window(Scroller::Hand).scrolls();
                        let hold = match (self.holdable(slot, element.place), scrolls) {
                            (false, _) => Hold::Never,
                            (true, false) => Hold::Drag,
                            (true, true) => Hold::UpOrDown,
                        };
                        (self.show.pointer_mut()).press(slot, pointer.position, hold);
                        if pointer.kind == PointerKind::Touch {
                            self.touch_pressed();
                        }
                    }
                }
            }
            PointerPhase::Move if self.layer.edited_id().is_some() => {
                self.edit(Edit::Move(pointer.position));
            }
            PointerPhase::Up if primary => {
                if self.layer.edited_id().is_some() {
                    self.edit(Edit::Up);
                }
                match self.gesture.take_release() {
                    Some(Release::Pile(pile))
                        if let Some(index) = hit(&self.frame, pointer.position)
                            && let place = self.frame.resolved[index as usize].place
                            && (self.pile_at(place).or_else(|| self.table_library_at(place)))
                                == Some(pile) =>
                    {
                        if !self.table_library_clicked(pile) {
                            self.dispatch_press(&Press::View(pile));
                        }
                    }
                    Some(Release::Element(pressed))
                        if let Some((id, press, true)) =
                            self.layer.press_at(pointer.position, open)
                            && pressed == id =>
                    {
                        self.press(&press);
                    }
                    _ => {}
                }
            }
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
                if self.layer.scroll(id, -pointer.wheel) {
                    self.place_shown();
                }
                true
            }
            PointerPhase::Down if pointer.kind == PointerKind::Touch => {
                if let Some(id) = self.layer.scroller_at(pointer.position, open) {
                    self.gesture = Gesture::Scroll {
                        finger: pointer.id,
                        id,
                        last: pointer.position,
                        release: None,
                    };
                }
                false
            }
            PointerPhase::Move => match &mut self.gesture {
                Gesture::Scroll {
                    finger,
                    id,
                    last,
                    release,
                } if *finger == pointer.id => {
                    // The content follows the finger, and the touch is no longer a press.
                    let delta = *last - pointer.position;
                    *last = pointer.position;
                    *release = None;
                    let id = *id;
                    if self.layer.scroll(id, delta) {
                        self.place_shown();
                    }
                    true
                }
                _ => false,
            },
            PointerPhase::Down | PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave => {
                false
            }
        }
    }

    /// A key goes to the screen's shortcuts first, then moves the focus (Tab), moves through
    /// or presses a row of the focused list or menu (`Layer::navigate`), moves a focused
    /// slider (left and right), presses the focused element (Return) or closes the menu or
    /// sheet on top (Escape), and otherwise opens the bug report on its chord: Cmd+B, or
    /// Cmd+Shift+B while the manual table is shown, where Cmd+B is the table's own
    /// (`isBugReportShortcut` in `ui/src/components/bugReportKeys.ts`). A key that means none
    /// of these waits for the next frame, as before.
    pub(super) fn key(&mut self, key: Key) {
        if key.phase == KeyPhase::Up || self.bug_key(key) {
            return;
        }
        if self.table_overlay_key(key) {
            return;
        }
        if self.layer.edited_id().is_some() && self.field_key(key) {
            return;
        }
        if let Some(press) = self.key_press(key) {
            self.press(&press);
        }
    }

    /// What `key` presses, in the order `key` describes, after it has moved the focus or the
    /// highlighted row of a list or menu if it does that instead; a key that means none of
    /// these is kept for the next frame.
    fn key_press(&mut self, key: Key) -> Option<Action> {
        let mac = self.platform == Platform::Mac;
        let first = key.phase == KeyPhase::Down;
        if first && key.code == KeyCode::Escape && self.table_escape() {
            return None;
        }
        if let Some(press) = self.layer.shortcut(&key, mac) {
            // A table command's key acts once for each press, not again as it repeats. While a
            // context menu is open it does nothing, and the menu's own keys move through its
            // rows.
            let table_key = press
                .downcast_ref::<crate::manual::TableCommand>()
                .is_some()
                || matches!(press.downcast_ref::<Press>(), Some(Press::Command(..)));
            let free = self.manual.menu.is_none() && !self.manual.overlay_open();
            if !table_key || (first && free) {
                return Some(press);
            }
            if free {
                return None;
            }
        }
        if key.code == KeyCode::Tab {
            self.layer.move_focus(!key.modifiers.shift);
            return None;
        }
        match self.layer.navigate(&key) {
            Navigation::Press(press) => return Some(press),
            Navigation::Moved => return None,
            Navigation::Ignored => {}
        }
        let arrow = match key.code {
            KeyCode::ArrowLeft => Some(false),
            KeyCode::ArrowRight => Some(true),
            _ => None,
        };
        if let Some(up) = arrow
            && let Some(press) = self.nudge(up)
        {
            return Some(press);
        }
        if first && key.code == KeyCode::Enter {
            return self.layer.focused_press();
        }
        if first && key.code == KeyCode::Escape {
            return self.layer.close_press();
        }
        let bug_report = if self.screen == Screen::Table {
            "Cmd+Shift+B"
        } else {
            "Cmd+B"
        };
        if first && Chord::parse(bug_report, mac) == Some(Chord::of(&key)) {
            return Some(Action::new(ScreenPress::Sheet(Some(Sheet::BugReport))));
        }
        // The platform's undo takes back an answer, as the web's `StageBoard` lets it.
        if first
            && self.screen == Screen::Game
            && Chord::parse("Cmd+Z", mac) == Some(Chord::of(&key))
        {
            return Some(Action::new(ScreenPress::Undo));
        }
        self.inputs.push(Input::Key(key));
        None
    }

    /// The press that moves the focused slider's level up a step, or down one if not `up`,
    /// as the left and right keys move a focused range input; `None` with no slider focused
    /// or at the end it would move past.
    fn nudge(&self, up: bool) -> Option<Action> {
        let action = self.layer.focused_slider()?;
        let setting = action.downcast_ref::<Setting>()?;
        let level = setting.level()?;
        let to = if up {
            level.saturating_add(STEP).min(100)
        } else {
            level.saturating_sub(STEP)
        };
        (to != level).then(|| Action::new(setting.at_level(to)))
    }

    /// Whether a press may pick up the card in `slot`, resting at `place`: a card the open
    /// prompt marks, once the prompt is drawn, in the viewer's hand or held up in a tray of two
    /// piles. This is the web interface's `#lifts` in `ui/src/stage/prompt.ts`.
    pub(super) fn holdable(&self, slot: usize, place: Place) -> bool {
        if let Some(holdable) = self.table_holdable(slot, place) {
            return holdable;
        }
        let Some((_, prompt)) = self.prompt.as_ref().filter(|_| self.open) else {
            return false;
        };
        let in_hand = matches!(
            place,
            Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                ..
            }
        );
        let on = u16::try_from(slot).map(|slot| Subject::Object {
            slot: ObjectId(slot),
        });
        // A card in a tray of piles is carried from pile to pile, as the web's `#piled` has it.
        let piled = (self.show.table().held()).is_some_and(|held| held.piles.len() > 1)
            && self.show.table().in_tray(slot);
        (in_hand || piled) && (prompt.marks.iter()).any(|marked| Ok(marked.on) == on)
    }

    /// The card in `slot`, held up in the open prompt's tray of piles, was let go at `at`: if it
    /// lands in a pile that is not closed, the prompt is told it was dropped there, before the
    /// card it landed in front of (`table::landing`), as the web's `#dropped` does. Let go
    /// outside every box, or in a closed pile, it goes back where it lay and nothing is sent.
    pub(super) fn drop_held(&mut self, slot: usize, at: Vec2) {
        let Some((_, prompt)) = self.prompt.as_ref().filter(|_| self.open) else {
            return;
        };
        let (ShapeBody::Split { piles }, Some(held)) = (&prompt.body, prompts::Piles::of(prompt))
        else {
            return;
        };
        let (table, size) = (self.show.table(), self.window.size);
        let held = held.held(table.held());
        let Ok(card) = u16::try_from(slot) else {
            return;
        };
        let landed = table::landing(&held, table.layout().tray, size, at, ObjectId(card));
        let Some((pile, before)) = landed else {
            return;
        };
        if piles.get(pile).is_none_or(|pile| pile.closed) {
            return;
        }
        if let (Ok(slot), Ok(pile), Ok(before)) = (
            u16::try_from(slot),
            u8::try_from(pile),
            u16::try_from(before),
        ) {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(Click::Drop {
                on,
                pile,
                at: before,
            });
        }
    }
}

/// Whether `pointer` ends what the primary pointer was doing: the primary button or a finger
/// coming up, Cancel or Leave.
fn ends_gesture(pointer: &Pointer) -> bool {
    match pointer.phase {
        PointerPhase::Up => pointer.button == Button::Primary,
        PointerPhase::Cancel | PointerPhase::Leave => true,
        PointerPhase::Down | PointerPhase::Move | PointerPhase::Wheel => false,
    }
}
