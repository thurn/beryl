//! Interprets mouse, pen and touch input and keeps the source of a gesture captured while
//! the pointer crosses other nodes. Features decide what to do with the resulting events.

use glam::Vec2;
use kit_base::data::pointer::PointerTuning;
use kit_frame::frame::{Frame, Resolved, StateBits};
use kit_ui::input::{Button, Pointer, PointerKind, PointerPhase};

use crate::hit::hit_where;
use crate::touch::{Landed, Touch};

/// A stable node and its containing region, if it belongs to one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target<Node, Container> {
    pub node: Node,
    pub container: Option<Container>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Start,
    Move,
    Drop,
}

/// The source stays captured from the press. The destination is what the current hit reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag<T> {
    pub source: Option<T>,
    pub destination: Option<T>,
    pub origin: Vec2,
    pub direction: Direction,
    pub stage: Stage,
    /// A touch that moved before its pickup delay can scroll but cannot pick an item up.
    pub pickup_ready: bool,
}

/// The toolkit's interpretation of one routed pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point<T> {
    pub pointer: Pointer,
    pub target: Option<T>,
    pub pressed: Option<T>,
    pub drag: Option<Drag<T>>,
    /// Whether this pointer owns the gesture that began with the primary press.
    pub captured: bool,
    /// The frame index when the scene target came from the drawn table.
    pub hit: Option<u32>,
}

impl<T: Copy> Point<T> {
    /// A primary release on the captured node.
    pub fn released_by<K: Eq>(self, node: impl Fn(T) -> K) -> Option<T> {
        (self.pointer.phase == PointerPhase::Up
            && self.pointer.button == Button::Primary
            && self.target.map(&node) == self.pressed.map(node))
        .then_some(self.pressed)
        .flatten()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TouchPolicy {
    /// A single-finger tap here can participate in a double tap.
    pub double_tap: bool,
    pub hold: bool,
    pub delayed_pickup: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Routed<T> {
    pub point: Option<Point<T>>,
    pub cancelled: bool,
    pub double_tap: bool,
}

#[derive(Clone, Copy, Debug)]
struct Capture<T> {
    source: Option<T>,
    pointer: (PointerKind, u8),
    origin: Vec2,
    direction: Option<Direction>,
    pickup_ready: bool,
    press_enabled: bool,
}

pub struct Router<T> {
    active: Option<Capture<T>>,
    touch: Touch,
    mouse: Option<Vec2>,
}

impl<T> Default for Router<T> {
    fn default() -> Self {
        Self {
            active: None,
            touch: Touch::default(),
            mouse: None,
        }
    }
}

impl<T: Copy> Router<T> {
    /// Hit tests the frame and supplies that result to the host's node mapping. The host can
    /// also resolve freshly laid out controls which have not been drawn into a frame yet.
    pub fn point(
        &mut self,
        frame: &Frame,
        pointer: Pointer,
        now: u64,
        tuning: &PointerTuning,
        target: impl FnOnce(Option<&Resolved>) -> (Option<T>, TouchPolicy),
    ) -> Routed<T> {
        self.route(pointer, now, tuning, |_, dragging| {
            let hit = hit_where(frame, pointer.position, |node| {
                !dragging || !node.state.has(StateBits::HELD)
            });
            let (target, policy) = target(hit.map(|index| &frame.resolved[index as usize]));
            (target, hit, policy)
        })
    }

    /// Recognizes a gesture using the target and touch policy supplied by the scene picker.
    pub fn route(
        &mut self,
        pointer: Pointer,
        now: u64,
        tuning: &PointerTuning,
        resolve: impl FnOnce(Option<T>, bool) -> (Option<T>, Option<u32>, TouchPolicy),
    ) -> Routed<T> {
        let active = self
            .active
            .filter(|active| active.pointer == (pointer.kind, pointer.id));
        let dragging = matches!(pointer.phase, PointerPhase::Move | PointerPhase::Up)
            && active.is_some_and(|capture| {
                capture.direction.is_some()
                    || (pointer.position - capture.origin).length() >= tuning.drag_threshold_points
            });
        let (target, hit, policy) = resolve(active.and_then(|capture| capture.source), dragging);
        let (accepted, cancelled, double_tap) = self.admit(pointer, policy, now, tuning);
        Routed {
            point: accepted.then(|| self.recognize(pointer, target, hit, policy, tuning)),
            cancelled,
            double_tap,
        }
    }

    fn admit(
        &mut self,
        pointer: Pointer,
        policy: TouchPolicy,
        now: u64,
        tuning: &PointerTuning,
    ) -> (bool, bool, bool) {
        if pointer.kind != PointerKind::Touch {
            return (true, false, false);
        }
        let (id, at) = (pointer.id, pointer.position);
        match pointer.phase {
            PointerPhase::Down => match self.touch.down(id, at, now, policy.double_tap, tuning) {
                Landed::Active => {
                    if policy.hold {
                        self.touch.wait_for_hold();
                    }
                    if policy.delayed_pickup {
                        self.touch.wait_for_pickup();
                    }
                    (true, false, false)
                }
                Landed::Together => {
                    self.active = None;
                    (false, true, false)
                }
                Landed::Ignored => (false, false, false),
            },
            PointerPhase::Move => (
                self.touch.moved(id, at, tuning.touch.slop_points),
                false,
                false,
            ),
            PointerPhase::Up => {
                let (active, double) = self.touch.up(id, now, tuning);
                (active, false, double)
            }
            PointerPhase::Cancel | PointerPhase::Leave => (self.touch.cancel(id), false, false),
            PointerPhase::Wheel => (false, false, false),
        }
    }

    fn recognize(
        &mut self,
        pointer: Pointer,
        target: Option<T>,
        hit: Option<u32>,
        policy: TouchPolicy,
        tuning: &PointerTuning,
    ) -> Point<T> {
        use PointerPhase::{Cancel, Down, Leave, Move, Up};

        if pointer.kind == PointerKind::Mouse {
            self.mouse = (!matches!(pointer.phase, Cancel | Leave)).then_some(pointer.position);
        }
        let source = (pointer.kind, pointer.id);
        let primary = pointer.button == Button::Primary;
        if pointer.phase == Down && primary {
            self.active = Some(Capture {
                source: target,
                pointer: source,
                origin: pointer.position,
                direction: None,
                press_enabled: true,
                pickup_ready: pointer.kind != PointerKind::Touch || !policy.delayed_pickup,
            });
        }
        let active = self
            .active
            .as_mut()
            .filter(|active| active.pointer == source);
        let captured = active.is_some();
        let pressed = active
            .as_ref()
            .filter(|active| active.press_enabled)
            .and_then(|active| active.source);
        let drag = match (pointer.phase, active) {
            (Move, Some(active)) => {
                let start = active.direction.is_none();
                let drag = drag(
                    active,
                    pointer.position,
                    target,
                    tuning,
                    if start { Stage::Start } else { Stage::Move },
                );
                if start && drag.is_some() && pointer.kind == PointerKind::Touch {
                    self.touch.moved_past_threshold();
                }
                drag
            }
            (Up, Some(active)) if primary => {
                drag(active, pointer.position, target, tuning, Stage::Drop)
            }
            _ => None,
        };
        if captured && (matches!(pointer.phase, Cancel | Leave) || (pointer.phase == Up && primary))
        {
            self.active = None;
        }
        Point {
            pointer,
            target,
            pressed,
            drag,
            captured,
            hit,
        }
    }

    pub fn pressed(&self) -> Option<T> {
        self.active
            .filter(|active| active.press_enabled)
            .and_then(|active| active.source)
    }

    pub fn mouse(&self) -> Option<Vec2> {
        self.mouse
    }

    /// Starts touch timers on the first frame after Down, then delivers a long press to the
    /// captured source. A long press can open a menu and cancel the underlying interaction.
    pub fn advance(&mut self, now: u64, tuning: &PointerTuning) -> Option<(Option<T>, Vec2)> {
        self.touch.frame(now);
        if self.touch.take_pickup(now, tuning)
            && let Some(active) = &mut self.active
        {
            active.pickup_ready = true;
        }
        let at = self.touch.take_hold(now, tuning)?;
        Some((self.active?.source, at))
    }

    pub fn due(&self, tuning: &PointerTuning) -> Option<u64> {
        self.touch.due(tuning)
    }

    pub fn cancel_touch(&mut self) -> bool {
        if !self.touch.reset() {
            return false;
        }
        self.active = None;
        true
    }

    /// Prevents a release from activating the pressed node after another control claims it.
    pub fn suppress_press(&mut self) {
        if let Some(active) = &mut self.active {
            active.press_enabled = false;
        }
    }

    pub fn cancel(&mut self) {
        self.active = None;
    }
}

fn drag<T: Copy>(
    capture: &mut Capture<T>,
    at: Vec2,
    destination: Option<T>,
    tuning: &PointerTuning,
    stage: Stage,
) -> Option<Drag<T>> {
    let travel = at - capture.origin;
    let direction = capture.direction.or_else(|| {
        (travel.length() >= tuning.drag_threshold_points).then(|| {
            if travel.x.abs() > travel.y.abs() {
                Direction::Horizontal
            } else {
                Direction::Vertical
            }
        })
    })?;
    capture.direction = Some(direction);
    Some(Drag {
        source: capture.source,
        destination,
        origin: capture.origin,
        direction,
        stage,
        pickup_ready: capture.pickup_ready,
    })
}

/// A focused control or a shortcut consumed the key, optionally with a message.
pub enum KeyResponse<T> {
    Handled,
    Action(T),
}

/// Focus owns the first opportunity to handle a key. An ignored key can reach a shortcut.
pub fn key<C, T>(
    context: &mut C,
    focused: impl FnOnce(&mut C) -> Option<KeyResponse<T>>,
    shortcut: impl FnOnce(&mut C) -> Option<KeyResponse<T>>,
) -> Option<KeyResponse<T>> {
    focused(context).or_else(|| shortcut(context))
}
