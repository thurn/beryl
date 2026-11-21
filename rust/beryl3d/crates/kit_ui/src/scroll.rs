//! Wheel, drag, keyboard and touch scrolling for clipped elements and arranged containers.
//! A viewport reports its offset and bounds in logical points or whole groups. The owner
//! applies the returned offset. Momentum samples an offset track on the caller's frame clock.

use glam::Vec2;
use kit_base::data::space::pointer::{
    DRAG_THRESHOLD_POINTS, SCROLL_STEP_POINTS, WHEEL_NOTCH_POINTS,
};
use kit_base::geometry::{GroupWindow, ScrollAxis};

use crate::input::{Button, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase};

use kit_base::data::space::pointer::scroll;

const NS_PER_MS: u64 = 1_000_000;
const COAST_NS: u64 = scroll::COAST_MS as u64 * NS_PER_MS;
const SAMPLE_NS: u64 = scroll::SAMPLE_MS as u64 * NS_PER_MS;
const STALE_NS: u64 = scroll::STALE_MS as u64 * NS_PER_MS;
const NS_PER_SECOND: f32 = 1_000_000_000.0;

/// The bounds and current offset of a scroller, plus the amount visible for page keys.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    offset: Vec2,
    limit: Vec2,
    page: Vec2,
    grouped: bool,
}

impl Viewport {
    #[must_use]
    pub fn points(offset: Vec2, limit: Vec2, page: Vec2) -> Self {
        Self {
            offset,
            limit,
            page,
            grouped: false,
        }
    }

    /// A viewport over the whole groups an arrangement shows.
    #[must_use]
    pub fn groups(window: GroupWindow) -> Self {
        let along = match window.axis {
            ScrollAxis::Horizontal => Vec2::X,
            ScrollAxis::Vertical => Vec2::Y,
        };
        Self {
            offset: along * window.first as f32,
            limit: along * window.last as f32,
            page: along * window.shown as f32,
            grouped: true,
        }
    }

    fn axes(self) -> Vec2 {
        Vec2::select(self.limit.cmpgt(Vec2::ZERO), Vec2::ONE, Vec2::ZERO)
    }

    fn clamp(self, offset: Vec2) -> Vec2 {
        offset.clamp(Vec2::ZERO, self.limit)
    }

    /// Whether this wheel event can scroll one of the viewport's axes.
    #[must_use]
    pub fn takes_wheel(self, wheel: Vec2) -> bool {
        self.wheel(wheel) != Vec2::ZERO
    }

    fn wheel(self, wheel: Vec2) -> Vec2 {
        if self.grouped {
            self.axes() * -(wheel.x + wheel.y) / WHEEL_NOTCH_POINTS
        } else {
            -wheel * self.axes()
        }
    }

    /// The requested offset for a plain scrolling key on this viewport.
    #[must_use]
    pub fn key(self, key: Key) -> Option<Vec2> {
        if key.phase == KeyPhase::Up || key.modifiers != Modifiers::default() {
            return None;
        }
        let step = if self.grouped {
            1.0
        } else {
            scroll::KEY_STEP_POINTS
        };
        let vertical = if self.limit.y > 0.0 { Vec2::Y } else { Vec2::X };
        let delta = match key.code {
            KeyCode::ArrowLeft => -Vec2::X * step,
            KeyCode::ArrowRight => Vec2::X * step,
            KeyCode::ArrowUp => -Vec2::Y * step,
            KeyCode::ArrowDown => Vec2::Y * step,
            KeyCode::PageUp => -vertical * self.page,
            KeyCode::PageDown => vertical * self.page,
            KeyCode::Home => -vertical * self.limit,
            KeyCode::End => vertical * self.limit,
            _ => return None,
        };
        (delta * self.axes() != Vec2::ZERO).then(|| self.clamp(self.offset + delta))
    }

    fn offset_for_travel(self, origin: Vec2, travel: Vec2) -> Vec2 {
        let units = if self.grouped {
            (travel / SCROLL_STEP_POINTS).trunc()
        } else {
            travel
        };
        self.clamp(origin + units)
    }
}

/// Whether a drag first passed the drag threshold sideways. Card pickup uses the same
/// threshold and direction so that a hand drag belongs to either scrolling or pickup.
#[must_use]
pub fn sideways(start: Vec2, at: Vec2) -> Option<bool> {
    let travel = at - start;
    (travel.length() >= DRAG_THRESHOLD_POINTS).then(|| travel.x.abs() > travel.y.abs())
}

/// A pointer event's effect on scrolling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollInput<T> {
    /// The event may still press or drag the content.
    Pass,
    /// Scrolling claimed the event, with a new offset if it moved.
    Claimed(Option<(T, Vec2)>),
}

#[derive(Clone, Copy, Debug)]
struct Drag<T> {
    target: T,
    pointer: (PointerKind, u8),
    start: Vec2,
    last: Vec2,
    sampled: u64,
    origin: Vec2,
    velocity: Vec2,
    claimed: bool,
}

#[derive(Clone, Copy, Debug)]
struct Momentum<T> {
    target: T,
    since: u64,
    origin: Vec2,
    travel: Vec2,
    distance: Vec2,
}

/// One active pointer scroll, the remainder of a partial wheel notch, and a touch's coast.
#[derive(Debug)]
pub struct ScrollGesture<T> {
    drag: Option<Drag<T>>,
    wheel: Option<(T, Vec2)>,
    momentum: Option<Momentum<T>>,
}

impl<T> Default for ScrollGesture<T> {
    fn default() -> Self {
        Self {
            drag: None,
            wheel: None,
            momentum: None,
        }
    }
}

impl<T: Copy + Eq> ScrollGesture<T> {
    pub fn cancel(&mut self) {
        self.drag = None;
        self.momentum = None;
    }

    /// Whether another frame should sample a moving touch or its momentum.
    #[must_use]
    pub fn moving(&self, now: u64) -> bool {
        self.momentum.is_some()
            || self.drag.is_some_and(|drag| {
                drag.claimed
                    && drag.pointer.0 == PointerKind::Touch
                    && now.saturating_sub(drag.sampled) < STALE_NS
            })
    }

    /// Takes `at` under a press or wheel event and looks up the current viewport of a drag.
    pub fn pointer(
        &mut self,
        pointer: &Pointer,
        now: u64,
        at: Option<(T, Viewport)>,
        view: impl FnOnce(T) -> Option<Viewport>,
    ) -> ScrollInput<T> {
        match pointer.phase {
            PointerPhase::Wheel => self.wheel(pointer, at),
            PointerPhase::Down if pointer.button == Button::Primary => {
                self.cancel();
                self.drag = at.map(|(target, view)| Drag {
                    target,
                    pointer: (pointer.kind, pointer.id),
                    start: pointer.position,
                    last: pointer.position,
                    sampled: now,
                    origin: view.offset,
                    velocity: Vec2::ZERO,
                    claimed: false,
                });
                ScrollInput::Pass
            }
            PointerPhase::Move | PointerPhase::Up | PointerPhase::Cancel | PointerPhase::Leave => {
                let Some(mut drag) = self
                    .drag
                    .filter(|drag| drag.pointer == (pointer.kind, pointer.id))
                else {
                    return ScrollInput::Pass;
                };
                if pointer.phase == PointerPhase::Move {
                    let Some(view) = view(drag.target) else {
                        self.drag = None;
                        return drag.input(None);
                    };
                    if !drag.claimed {
                        let axes = view.axes();
                        if pointer.kind == PointerKind::Touch && !view.grouped {
                            // A list follows even a small finger move. Once it moves, lifting
                            // the finger must not press the row that moved with it.
                            if (pointer.position - drag.start) * axes == Vec2::ZERO {
                                return ScrollInput::Pass;
                            }
                        } else {
                            let Some(horizontal) = sideways(drag.start, pointer.position) else {
                                return ScrollInput::Pass;
                            };
                            if (horizontal && axes.x == 0.0) || (!horizontal && axes.y == 0.0) {
                                self.drag = None;
                                return ScrollInput::Pass;
                            }
                        }
                        drag.claimed = true;
                    }
                    let offset = drag.moved(pointer.position, now, view);
                    self.drag = Some(drag);
                    return drag.input(offset);
                }
                if pointer.phase == PointerPhase::Up && pointer.button != Button::Primary {
                    return ScrollInput::Pass;
                }
                self.drag = None;
                if !drag.claimed {
                    return ScrollInput::Pass;
                }
                if pointer.phase == PointerPhase::Up
                    && pointer.kind == PointerKind::Touch
                    && now.saturating_sub(drag.sampled) < STALE_NS
                    && drag.velocity != Vec2::ZERO
                {
                    self.momentum = Some(Momentum {
                        target: drag.target,
                        since: now,
                        origin: drag.origin,
                        travel: drag.start - drag.last,
                        // The quadratic ease-out starts at the release velocity and ends at rest.
                        distance: drag.velocity * (0.5 * COAST_NS as f32 / NS_PER_SECOND),
                    });
                }
                ScrollInput::Claimed(None)
            }
            PointerPhase::Down => ScrollInput::Pass,
        }
    }

    fn wheel(&mut self, pointer: &Pointer, at: Option<(T, Viewport)>) -> ScrollInput<T> {
        let Some((target, view)) = at.filter(|(_, view)| view.takes_wheel(pointer.wheel)) else {
            return ScrollInput::Pass;
        };
        self.cancel();
        let delta = view.wheel(pointer.wheel);
        let steps = if view.grouped {
            let (key, rest) = self.wheel.unwrap_or((target, Vec2::ZERO));
            let travel = if key == target { rest + delta } else { delta };
            let steps = travel.trunc();
            self.wheel = Some((target, travel - steps));
            steps
        } else {
            self.wheel = None;
            delta
        };
        let offset = view.clamp(view.offset + steps);
        ScrollInput::Claimed((offset != view.offset).then_some((target, offset)))
    }

    /// Samples the track at `now` and stops as soon as it reaches the bounds or comes to rest.
    pub fn advance(
        &mut self,
        now: u64,
        view: impl FnOnce(T) -> Option<Viewport>,
    ) -> Option<(T, Vec2)> {
        let motion = self.momentum?;
        let Some(view) = view(motion.target) else {
            self.momentum = None;
            return None;
        };
        let progress = (now.saturating_sub(motion.since) as f32 / COAST_NS as f32).min(1.0);
        let offset = view.offset_for_travel(
            motion.origin,
            motion.travel + motion.distance * progress * (2.0 - progress),
        );
        let end = view.offset_for_travel(motion.origin, motion.travel + motion.distance);
        if progress >= 1.0 || offset == end {
            self.momentum = None;
        }
        (offset != view.offset).then_some((motion.target, offset))
    }
}

impl<T: Copy> Drag<T> {
    fn input(self, offset: Option<Vec2>) -> ScrollInput<T> {
        if self.claimed {
            ScrollInput::Claimed(offset.map(|offset| (self.target, offset)))
        } else {
            ScrollInput::Pass
        }
    }

    fn moved(&mut self, at: Vec2, now: u64, view: Viewport) -> Option<Vec2> {
        let axes = view.axes();
        let travel = (self.last - at) * axes;
        if travel != Vec2::ZERO {
            let elapsed = now.saturating_sub(self.sampled).max(SAMPLE_NS) as f32;
            self.velocity = (travel * (NS_PER_SECOND / elapsed))
                .clamp_length_max(scroll::MAX_POINTS_PER_SECOND);
            self.sampled = now;
        }
        self.last = at;
        // Point offsets follow each move, so reversing at a bound moves immediately.
        if !view.grouped {
            self.origin = view.clamp(view.offset + travel);
            self.start = at;
        }
        let offset = view.offset_for_travel(self.origin, self.start - at);
        (offset != view.offset).then_some(offset)
    }
}
