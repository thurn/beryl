//! Pointer gestures given at points of the window rather than at elements: the wheel, and a
//! drag between two points.

use glam::Vec2;

use super::{DRAG_MOVES, DRAG_STEP, Page};
use crate::input::{Button, Input, Modifiers, Pointer, PointerKind, PointerPhase};

impl Page {
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
        self.pointer(PointerPhase::Down, from, Button::Primary);
        for move_number in 1..=DRAG_MOVES {
            let point = from.lerp(to, move_number as f32 / DRAG_MOVES as f32);
            self.pointer(PointerPhase::Move, point, Button::Primary);
            self.step(DRAG_STEP);
        }
        self.pointer(PointerPhase::Up, to, Button::Primary);
        self.render();
    }
}
