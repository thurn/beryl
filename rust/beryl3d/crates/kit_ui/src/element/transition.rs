//! Timing and visual values declared on an element's style.

use glam::Vec2;
use kit_base::pose::Curve;

/// The part of an element's appearance that can change over time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Opacity,
    Translation,
    Scale,
    Color,
    Background,
    /// The width of an anchored or absolutely positioned element.
    Width,
    /// The height of an anchored or absolutely positioned element.
    Height,
}

/// Timing on the client's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub duration_ms: f32,
    pub delay_ms: f32,
    pub curve: Curve,
}

impl Timing {
    #[must_use]
    pub const fn new(duration_ms: f32, delay_ms: f32, curve: Curve) -> Self {
        Self {
            duration_ms,
            delay_ms,
            curve,
        }
    }

    /// The time before interpolation and the time spent interpolating, in nanoseconds.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "nonnegative milliseconds converted to whole nanoseconds"
    )]
    pub(crate) fn nanoseconds(self) -> (u64, u64) {
        (
            (self.delay_ms.max(0.0) * 1e6) as u64,
            (self.duration_ms.max(0.0) * 1e6) as u64,
        )
    }
}

/// How a change to one style property is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub property: Property,
    pub timing: Timing,
}

impl Transition {
    #[must_use]
    pub const fn new(property: Property, timing: Timing) -> Self {
        Self { property, timing }
    }
}

/// The values a node enters from or leaves toward. An omitted value keeps its style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Presence {
    pub timing: Timing,
    pub opacity: Option<f32>,
    pub translation: Option<Vec2>,
    pub scale: Option<f32>,
    /// Whether direct children with a grouped entrance use their parent's entrance instead.
    pub grouped: bool,
}

impl Presence {
    #[must_use]
    pub const fn new(timing: Timing) -> Self {
        Self {
            timing,
            opacity: None,
            translation: None,
            scale: None,
            grouped: false,
        }
    }

    #[must_use]
    pub const fn fade(timing: Timing) -> Self {
        Self {
            opacity: Some(0.0),
            ..Self::new(timing)
        }
    }
}
