//! Carrying a waiting trigger along an order prompt's row (`prompts::order`), as the web
//! interface's `OrderRow` (`ui/src/stage/Tray.tsx`) does: the primary pointer goes down on an
//! item, and once it has travelled the drag threshold the item leaves the row and a float draws
//! it under the pointer, moved by the pointer alone, so that a carry allocates nothing after it
//! begins. Let go, the item is dropped before the item it lands in front of. A press that never
//! travelled the threshold is a click, which the flat layer presses as usual.

use beryl_protocol::prompt::{Click, ShapeBody};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::Client;
use super::input::{Control, ScenePoint};
use game_hud::prompts::order;
use kit_ui::element::floats;
use kit_ui::input::{Button, PointerPhase};
use kit_ui::layout::FloatLook;

/// An item of an order's row that the primary pointer went down on: which item, where the
/// press began, and whether it has travelled the drag threshold and left the row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Carry {
    pub(super) on: Subject,
    start: Vec2,
    pub(super) lifted: bool,
}

impl Client {
    pub(super) fn carried_item(&self) -> Option<Subject> {
        match self.routes.control {
            Control::Order(carry) => Some(carry.on),
            _ => None,
        }
    }

    /// Carries an item of the open order prompt's row by `pointer`, as the module describes.
    /// Returns whether the pointer carried an item, and so means nothing else.
    pub(super) fn carry_item(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        match pointer.phase {
            PointerPhase::Down if pointer.button == Button::Primary => {
                if let Some(on) = self.order_item_at(pointer.position) {
                    let carry = Carry {
                        on,
                        start: pointer.position,
                        lifted: false,
                    };
                    self.routes.control = Control::Order(carry);
                }
                false
            }
            PointerPhase::Move => {
                let Control::Order(carry) = self.routes.control else {
                    return false;
                };
                if !self.still_ordering(carry) {
                    self.cancel_gesture();
                    return false;
                }
                if !carry.lifted && point.drag.is_none() {
                    return false;
                }
                if !carry.lifted {
                    self.lift_item(carry);
                }
                self.move_item(carry, pointer.position);
                true
            }
            PointerPhase::Up if pointer.button == Button::Primary => {
                let Control::Order(carry) = self.routes.control else {
                    return false;
                };
                if !carry.lifted {
                    return false;
                }
                self.routes.control = Control::None;
                self.put_item_down();
                if !self.still_ordering(carry) {
                    return true;
                }
                let at = (self.features.prompts.current())
                    .and_then(|prompt| {
                        order::slot_at(prompt, self.show.table().profile(), pointer.position.x)
                    })
                    .and_then(|at| u16::try_from(at).ok());
                if let Some(at) = at {
                    self.click(Click::Drop {
                        on: carry.on,
                        pile: 0,
                        at,
                    });
                }
                true
            }
            _ => false,
        }
    }

    /// Whether the item `carry` names is still in the row of the order prompt open now, which
    /// takes presses. A reply or a view that arrives while the item is carried may replace the
    /// prompt; a drop is then not sent, as the web's `OrderRow` takes no carry while a move is
    /// being sent, and the prompt the session opens next is the one to answer.
    fn still_ordering(&self, carry: Carry) -> bool {
        (self.features.prompts.current())
            .filter(|_| self.features.layer_open)
            .is_some_and(|prompt| {
                order::cell_of(prompt, self.show.table().profile(), carry.on).is_some()
            })
    }

    /// The item of the open order prompt's row that a press at `point` reaches, if any.
    fn order_item_at(&self, point: Vec2) -> Option<Subject> {
        let prompt = self
            .features
            .prompts
            .current()
            .filter(|_| self.features.layer_open)?;
        if !matches!(prompt.body, ShapeBody::Order { .. }) {
            return None;
        }
        let (_, action, enabled) = self.layer.press_at(point, self.features.layer_open)?;
        match (order::click(&action)?, enabled) {
            (
                Click::Drop {
                    on: on @ Subject::Ability { .. },
                    ..
                },
                true,
            ) => Some(*on),
            _ => None,
        }
    }

    /// Takes the item `carry` names out of the row and shows it as the carried float, which
    /// stops the press under it from clicking it.
    fn lift_item(&mut self, carry: Carry) {
        self.routes.control = Control::Order(Carry {
            lifted: true,
            ..carry
        });
        let float = (self.features.prompts.current()).and_then(|prompt| {
            let view = self.show.latest().map(|update| &update.view);
            let faces = Some(self.show.faces().list());
            order::carried_item(prompt, carry.on, view, faces, self.show.table().profile())
        });
        self.layer.float(floats::CARRIED_ITEM, float);
        self.show_screen();
    }

    /// Moves the carried float so that the item keeps the place under the pointer, now at
    /// `at`, that it had where the press began.
    fn move_item(&mut self, carry: Carry, at: Vec2) {
        let Some(cell) = (self.features.prompts.current())
            .and_then(|prompt| order::cell_of(prompt, self.show.table().profile(), carry.on))
        else {
            return;
        };
        self.layer.set_float_look(
            floats::CARRIED_ITEM,
            FloatLook {
                point: Vec2::new(cell.x, cell.y) + (at - carry.start),
                ..FloatLook::default()
            },
        );
    }

    /// Takes the carried float away and puts its item back in the row, until the session's
    /// reply to the drop draws the row again.
    pub(super) fn put_item_down(&mut self) {
        self.layer.float(floats::CARRIED_ITEM, None);
        self.show_screen();
    }
}
