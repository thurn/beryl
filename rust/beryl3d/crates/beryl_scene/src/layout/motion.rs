//! Transitions of the flat layer's screen (docs/beryl-3d.md, "Elements"): when a kept node's
//! `opacity` or its laid-out place on the window changes, it moves or fades there over
//! `flat.transition.ms` in `data/beryl3d/space.json`, eased out as the web interface's action
//! dock comes in (`dock-in 0.22s ease-out` in `ui/src/screens/styles/controls.css`). A node
//! that enters the screen fades in over the same; a node that leaves is gone at once.
//!
//! A change is found in `Client::handle`, which has no clock, so a transition begins at the
//! next frame's time (`Layer::tick`), and every frame works out its values from the start and
//! end alone, allocating nothing. Floats move by their looks instead, and never transition.

use glam::Vec2;

use super::{Layer, Node};
use crate::data::space::flat::transition::MS;
use crate::pose::Curve;

/// A node's transition, from where and how opaque it was toward where its layout puts it and
/// its style's opacity.
#[derive(Clone, Copy, Debug)]
pub(super) struct Motion {
    /// When it began, in nanoseconds since the client started, once a frame has stamped it.
    start: Option<u64>,
    /// How far from its laid-out place it stood when it began.
    from_offset: Vec2,
    /// How opaque it was when it began, from 0 to 1.
    from_opacity: f32,
    /// Where it stands now, off its laid-out place, as the last frame worked it out.
    offset: Vec2,
    /// How opaque it is now, as the last frame worked it out.
    opacity: f32,
}

impl Motion {
    /// A transition that fades a node in from nothing.
    pub(super) fn fade_in() -> Self {
        Self {
            start: None,
            from_offset: Vec2::ZERO,
            from_opacity: 0.0,
            offset: Vec2::ZERO,
            opacity: 0.0,
        }
    }
}

impl Node {
    /// How far this node stands from its laid-out place now. It moves this node alone.
    pub(super) fn offset(&self) -> Vec2 {
        self.motion
            .filter(|motion| motion.start.is_some())
            .map_or(Vec2::ZERO, |motion| motion.offset)
    }

    /// How opaque this node is now, from 0 to 1, before its parent's opacity. A transition no
    /// frame has begun yet shows its end, so a layer written without a clock shows its layout.
    pub(super) fn opacity(&self) -> f32 {
        self.motion
            .filter(|motion| motion.start.is_some())
            .map_or(self.element.style.opacity, |motion| motion.opacity)
    }

    /// Whether no frame has drawn this node yet: it is fading in from nothing, and no frame has
    /// begun the fade.
    pub(super) fn unseen(&self) -> bool {
        self.motion
            .is_some_and(|motion| motion.start.is_none() && motion.from_opacity <= 0.0)
    }

    /// Begins a transition from where the node stands now and how opaque it is now, `moved`
    /// further from its laid-out place: the change in that place.
    pub(super) fn transition(&mut self, moved: Vec2, opacity: f32) {
        let offset = self.offset() + moved;
        self.motion = Some(Motion {
            start: None,
            from_offset: offset,
            from_opacity: opacity,
            offset,
            opacity,
        });
    }
}

impl Layer {
    /// Stamps the transitions found since the last frame with `now`, and works out where every
    /// node of the screen stands and how opaque it is at `now`. Returns whether any transition
    /// is still under way, and whether one began at `now`. A tooltip whose wait is over shows
    /// (`tip`). The client calls it at the start of every frame. Allocates nothing.
    pub fn tick(&mut self, now: u64) -> Ticked {
        fn visit(node: &mut Node, now: u64, began: &mut bool) -> bool {
            let mut moving = false;
            if let Some(motion) = &mut node.motion {
                *began |= motion.start.is_none();
                let start = *motion.start.get_or_insert(now);
                // A transition lasts a fraction of a second, far inside f32's exact range.
                #[expect(clippy::cast_precision_loss, reason = "under a second of nanoseconds")]
                let elapsed = now.saturating_sub(start) as f32 / 1e6 / MS;
                if elapsed >= 1.0 {
                    node.motion = None;
                } else {
                    let eased = Curve::EASE_OUT.ease(elapsed);
                    let to = node.element.style.opacity;
                    motion.offset = motion.from_offset * (1.0 - eased);
                    motion.opacity = motion.from_opacity + (to - motion.from_opacity) * eased;
                    moving = true;
                }
            }
            for child in &mut node.children {
                moving |= visit(child, now, began);
            }
            moving
        }
        self.tick_tip(now);
        let mut began = false;
        let moving = (self.screen.as_mut()).is_some_and(|node| visit(node, now, &mut began));
        Ticked { moving, began }
    }
}

/// How long a transition of the flat layer's screen lasts, in nanoseconds.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a fraction of a second, a whole number of nanoseconds"
)]
pub fn transition_nanoseconds() -> u64 {
    (MS * 1e6) as u64
}

/// What `Layer::tick` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticked {
    /// Whether a transition of the screen is still under way.
    pub moving: bool,
    /// Whether a transition began at this frame: a node entered the screen or its place or
    /// opacity changed since the last frame.
    pub began: bool,
}
