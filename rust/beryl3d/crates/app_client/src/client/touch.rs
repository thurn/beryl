//! The client's side of a touch screen (`pointer::touch`; the phone's gestures in
//! docs/ui-conventions.md, "The phone"):
//!
//! - Only the active finger reaches the game (`hear_touch`). A second finger landing with the
//!   first ends the first finger's press with nothing done.
//! - A tap on a card the open prompt can use uses it; a tap on any other card, the hand's
//!   included, shows it whole in the inspector, as the web's `Pointer` hands an unhandled click
//!   to its viewers (`#click` in `ui/src/stage/pointer.ts`). The next touch anywhere puts it
//!   away and means nothing else.
//! - A finger held still on a card for `LONG_PRESS_MS` shows that card whole on the game screen,
//!   and opens the context menu of what is under it on the manual table. Its lift then means
//!   nothing.
//! - A card in the hand can be carried off to play it as soon as the finger moves. On the manual
//!   table, where a hold opens a menu, a finger must rest `PICKUP_MS` on a card before it can
//!   pick the card up; moving sooner moves what is under it instead.
//! - A double tap on bare felt, or with two or more fingers anywhere, opens the bug report.

use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::Client;
use app_screens::screens::Screen;
use game_play::pointer::takes_pointer;
use game_play::pointer::touch::Landed;
use kit_frame::frame::Role;
use kit_scene::hit::hit;
use kit_ui::input::{Pointer, PointerPhase};

/// What the client makes of a finger's event (`Client::hear_touch`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Heard {
    /// The event goes on to the game: it is the active finger's.
    pub game: bool,
    /// The event finished a double tap, which opens the bug report.
    pub report: bool,
}

impl Client {
    /// Decides whether a finger's event reaches the game, as the module describes. A finger
    /// going down while the inspector shows a card it was tapped or held on puts the inspector
    /// away, and its press means nothing else.
    pub(super) fn hear_touch(&mut self, pointer: &Pointer) -> Heard {
        let (id, at) = (pointer.id, pointer.position);
        let mut report = false;
        let game = match pointer.phase {
            PointerPhase::Down => {
                let felt = self.on_felt(at);
                match self.touch.down(id, at, self.now, felt) {
                    Landed::Active if self.inspector.pinned.take().is_some() => {
                        self.inspector.due = true;
                        false
                    }
                    Landed::Active => {
                        if self.holds_at(at) {
                            self.touch.wait_for_hold();
                        }
                        true
                    }
                    Landed::Together => {
                        self.drop_touch_press();
                        false
                    }
                    Landed::Ignored => false,
                }
            }
            PointerPhase::Move => self.touch.moved(id, at),
            PointerPhase::Up => {
                let (active, double) = self.touch.up(id, self.now);
                report = double;
                active
            }
            PointerPhase::Cancel | PointerPhase::Leave => self.touch.cancel(id),
            PointerPhase::Wheel => false,
        };
        Heard { game, report }
    }

    /// The active finger has just pressed a card of the table: its release without moving is a
    /// tap, and on the manual table it waits `PICKUP_MS` before it may pick the card up.
    pub(super) fn touch_pressed(&mut self) {
        let delayed = self.screen == Screen::Table;
        self.show.pointer_mut().by_touch(delayed);
        if delayed {
            self.touch.wait_for_pickup();
        }
    }

    /// Ends every finger's gesture, as when the surface changes size or the screen changes: the
    /// card a finger pressed falls back and is not clicked, and no hold or pickup comes due.
    pub(super) fn cancel_touch(&mut self) {
        if self.touch.reset() {
            self.drop_touch_press();
        }
    }

    /// At the frame at `now`, starts timing the active finger's hold and pickup, lets its card
    /// be picked up once the pickup is due, and carries out its hold once that is due.
    pub(super) fn touch_frame(&mut self, now: u64) {
        self.touch.frame(now);
        if self.touch.take_pickup(now) {
            self.show.pointer_mut().ready();
        }
        let Some(at) = self.touch.take_hold(now) else {
            return;
        };
        let held = if self.screen == Screen::Table {
            self.open_table_menu_at(at)
        } else {
            self.inspect_at(at)
        };
        if held {
            self.show.pointer_mut().cancel();
            self.end_gesture();
        }
    }

    /// A press that picked nothing up on the card in `slot`: with Cmd held on the
    /// manual table it adds the card to the selection or takes it out, on a card the open
    /// decision refuses it says why (`refuse_click`), and otherwise it clicks the open prompt on
    /// the card.
    pub(super) fn card_clicked(&mut self, slot: usize) {
        if let Ok(slot) = u16::try_from(slot)
            && !self.table_click(ObjectId(slot))
            && !self.refuse_click(ObjectId(slot))
        {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(beryl_protocol::prompt::Click::On { on });
        }
    }

    /// A finger's tap on the card in `slot`: a click if the open prompt can use the card or the
    /// manual table is shown, and otherwise the card shown whole.
    pub(super) fn card_tapped(&mut self, slot: usize) {
        if self.screen != Screen::Game || self.usable(slot) {
            self.card_clicked(slot);
        } else if let Ok(slot) = u16::try_from(slot) {
            self.inspector.pinned = Some(ObjectId(slot));
            self.inspector.due = true;
        }
    }

    /// Whether the open prompt can use the card in `slot`: it marks the card.
    fn usable(&self, slot: usize) -> bool {
        let Some((_, prompt)) = self.prompt.as_ref().filter(|_| self.open) else {
            return false;
        };
        let Ok(slot) = u16::try_from(slot) else {
            return false;
        };
        let on = Subject::Object {
            slot: ObjectId(slot),
        };
        prompt.marks.iter().any(|marked| marked.on == on)
    }

    /// Shows whole the card of the table at `at`, on the game screen with no sheet over it.
    /// Returns whether there was one.
    fn inspect_at(&mut self, at: Vec2) -> bool {
        if self.screen != Screen::Game || self.sheet.is_some() {
            return false;
        }
        let Some(index) = hit(&self.frame, at) else {
            return false;
        };
        let element = &self.frame.resolved[index as usize];
        if element.role != Role::Card {
            return false;
        }
        let Ok(slot) = u16::try_from(element.id.0) else {
            return false;
        };
        self.inspector.pinned = Some(ObjectId(slot));
        self.inspector.due = true;
        true
    }

    /// Whether a finger held still at `at` means something: anywhere on the manual table with
    /// nothing over it, where it opens a context menu, and on a card of the game's table that no
    /// element of the flat layer covers, where it shows the card whole.
    fn holds_at(&self, at: Vec2) -> bool {
        match self.screen {
            Screen::Table => {
                self.sheet.is_none()
                    && self.manual.menu.is_none()
                    && (self.show.latest()).is_some_and(|update| update.view.table.is_some())
            }
            Screen::Game => {
                self.sheet.is_none()
                    && self.layer.press_at(at, self.open).is_none()
                    && hit(&self.frame, at).is_some_and(|index| {
                        let element = &self.frame.resolved[index as usize];
                        element.role == Role::Card && takes_pointer(element.place)
                    })
            }
            _ => false,
        }
    }

    /// Whether `at` is bare felt: the game's or the manual table's table with nothing drawn
    /// there, where a tap does nothing.
    fn on_felt(&self, at: Vec2) -> bool {
        matches!(self.screen, Screen::Game | Screen::Table)
            && self.sheet.is_none()
            && hit(&self.frame, at).is_none()
    }

    /// Ends the active finger's press with nothing done: a card it holds falls back, a row it
    /// drags stops, and the element of the flat layer it pressed is no longer drawn held down.
    fn drop_touch_press(&mut self) {
        self.show.pointer_mut().cancel();
        self.row_gesture.cancel();
        self.end_gesture();
        if self.manual.sweep.take().is_some() {
            self.show_screen();
        }
        self.layer.point_at(None, None);
    }
}
