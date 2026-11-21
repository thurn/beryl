//! Pointer gestures given at points of the window rather than at elements: the wheel, and a
//! drag between two points, let go or held; and the point of the window an action on a locator
//! acts at.

use crate::client;
use client::Input;
use glam::Vec2;

use super::{DRAG_MOVES, DRAG_STEP, Page, point};
use crate::agent::uncovered;
use crate::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

impl Page {
    /// Where an action on `locator`'s element happens: the centre of its largest uncovered
    /// rectangle, or the point itself when `locator` writes one (`action::point`).
    pub(super) fn point(&mut self, locator: &str) -> Vec2 {
        if let Some(at) = point(locator) {
            return at;
        }
        let element = self.find(locator);
        let frame = self.frame();
        let Some(rect) = uncovered(frame, element.index) else {
            let message = format!("no part of '{locator}' can be pressed: it is covered");
            panic!("{}", self.error(&message));
        };
        Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    /// Turns the mouse wheel at `point` by `wheel` logical points, as a host gives it: positive
    /// moves the content right and down, as a wheel turned up or to the left does.
    pub fn wheel(&mut self, point: Vec2, wheel: Vec2) {
        self.give(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Wheel,
            position: point,
            button: Button::None,
            wheel,
            modifiers: Modifiers::default(),
        }));
        self.render();
    }

    /// Presses at `from`, moves in eight even steps along the straight line to `to` with a
    /// 16 ms step after each, as `drag` does, and releases there.
    pub fn drag_between(&mut self, from: Vec2, to: Vec2) {
        self.hold_between(from, to);
        self.pointer(PointerPhase::Up, to, Button::Primary);
        self.render();
    }

    /// Presses at `from` and moves to `to` as `drag_between` does, but keeps the button down
    /// there, as the `hold` action does.
    pub fn hold_between(&mut self, from: Vec2, to: Vec2) {
        self.pointer(PointerPhase::Down, from, Button::Primary);
        for move_number in 1..=DRAG_MOVES {
            let point = from.lerp(to, move_number as f32 / DRAG_MOVES as f32);
            self.pointer(PointerPhase::Move, point, Button::Primary);
            self.step(DRAG_STEP);
        }
        self.render();
    }
}
