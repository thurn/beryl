//! Applies a recognized drag or wheel event to a scroll viewport and samples touch momentum.

use glam::Vec2;
use kit_base::data::pointer::PointerTuning;
use kit_ui::input::{Button, Pointer, PointerKind, PointerPhase};
use kit_ui::scroll::Viewport;

use crate::input::Direction;

const NS_PER_MS: u64 = 1_000_000;
const NS_PER_SECOND: f32 = 1_000_000_000.0;

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
    pub fn moving(&self, now: u64, tuning: &PointerTuning) -> bool {
        self.momentum.is_some()
            || self.drag.is_some_and(|drag| {
                drag.claimed
                    && drag.pointer.0 == PointerKind::Touch
                    && now.saturating_sub(drag.sampled)
                        < tuning.scroll.stale_ms.saturating_mul(NS_PER_MS)
            })
    }

    /// Takes `at` under a press or wheel event and looks up the current viewport of a drag.
    pub fn pointer(
        &mut self,
        pointer: &Pointer,
        direction: Option<Direction>,
        now: u64,
        at: Option<(T, Viewport)>,
        view: impl FnOnce(T) -> Option<Viewport>,
        tuning: &PointerTuning,
    ) -> ScrollInput<T> {
        match pointer.phase {
            PointerPhase::Wheel => self.wheel(pointer, at, tuning.wheel_notch_points),
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
                        if pointer.kind == PointerKind::Touch && !view.requires_direction() {
                            // A list follows even a small finger move. Once it moves, lifting
                            // the finger must not press the row that moved with it.
                            if (pointer.position - drag.start) * axes == Vec2::ZERO {
                                return ScrollInput::Pass;
                            }
                        } else {
                            let Some(direction) = direction else {
                                return ScrollInput::Pass;
                            };
                            let horizontal = direction == Direction::Horizontal;
                            if (horizontal && axes.x == 0.0) || (!horizontal && axes.y == 0.0) {
                                self.drag = None;
                                return ScrollInput::Pass;
                            }
                        }
                        drag.claimed = true;
                    }
                    let offset = drag.moved(pointer.position, now, view, tuning);
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
                    && now.saturating_sub(drag.sampled)
                        < tuning.scroll.stale_ms.saturating_mul(NS_PER_MS)
                    && drag.velocity != Vec2::ZERO
                {
                    self.momentum = Some(Momentum {
                        target: drag.target,
                        since: now,
                        origin: drag.origin,
                        travel: drag.start - drag.last,
                        // The quadratic ease-out starts at the release velocity and ends at rest.
                        distance: drag.velocity
                            * (0.5 * tuning.scroll.coast_ms.saturating_mul(NS_PER_MS) as f32
                                / NS_PER_SECOND),
                    });
                }
                ScrollInput::Claimed(None)
            }
            PointerPhase::Down => ScrollInput::Pass,
        }
    }

    fn wheel(
        &mut self,
        pointer: &Pointer,
        at: Option<(T, Viewport)>,
        notch: f32,
    ) -> ScrollInput<T> {
        let Some((target, view)) = at.filter(|(_, view)| view.takes_wheel(pointer.wheel, notch))
        else {
            return ScrollInput::Pass;
        };
        self.cancel();
        let delta = view.wheel(pointer.wheel, notch);
        let steps = if view.is_grouped() {
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
        tuning: &PointerTuning,
    ) -> Option<(T, Vec2)> {
        let motion = self.momentum?;
        let Some(view) = view(motion.target) else {
            self.momentum = None;
            return None;
        };
        let progress = (now.saturating_sub(motion.since) as f32
            / tuning.scroll.coast_ms.saturating_mul(NS_PER_MS) as f32)
            .min(1.0);
        let offset = view.offset_for_travel(
            motion.origin,
            motion.travel + motion.distance * progress * (2.0 - progress),
            tuning.scroll_step_points,
        );
        let end = view.offset_for_travel(
            motion.origin,
            motion.travel + motion.distance,
            tuning.scroll_step_points,
        );
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

    fn moved(
        &mut self,
        at: Vec2,
        now: u64,
        view: Viewport,
        tuning: &PointerTuning,
    ) -> Option<Vec2> {
        let axes = view.axes();
        let travel = (self.last - at) * axes;
        if travel != Vec2::ZERO {
            let elapsed =
                now.saturating_sub(self.sampled)
                    .max(tuning.scroll.sample_ms.saturating_mul(NS_PER_MS)) as f32;
            self.velocity = (travel * (NS_PER_SECOND / elapsed))
                .clamp_length_max(tuning.scroll.max_points_per_second);
            self.sampled = now;
        }
        self.last = at;
        // Point offsets follow each move, so reversing at a bound moves immediately.
        if !view.is_grouped() {
            self.origin = view.clamp(view.offset + travel);
            self.start = at;
        }
        let offset =
            view.offset_for_travel(self.origin, self.start - at, tuning.scroll_step_points);
        (offset != view.offset).then_some(offset)
    }
}
