//! Curves over a layer's clock, decoded once with the effect definition.

use serde::Deserialize;

use super::one;
use crate::data::tuning::{finite, fraction};
use crate::pose::Curve;

/// A constant, a ramp that can hold at either end, or a wave with a ramp for its amplitude.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Scalar {
    Constant(#[serde(deserialize_with = "finite")] f32),
    Ramp(Ramp),
    Wave(Wave),
}

impl Scalar {
    #[must_use]
    pub fn at(&self, progress: f32) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::Ramp(ramp) => ramp.at(progress),
            Self::Wave(wave) => {
                (std::f32::consts::TAU * wave.cycles * progress).sin() * wave.amplitude.at(progress)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    #[serde(deserialize_with = "finite")]
    from: f32,
    #[serde(deserialize_with = "finite")]
    by: f32,
    #[serde(default, deserialize_with = "fraction")]
    start: f32,
    #[serde(default = "one", deserialize_with = "fraction")]
    end: f32,
    #[serde(default)]
    ease: Ease,
}

impl Ramp {
    fn at(&self, progress: f32) -> f32 {
        let passed = if progress >= self.end {
            1.0
        } else if progress <= self.start {
            0.0
        } else {
            (progress - self.start) / (self.end - self.start)
        };
        self.from + self.by * self.ease.at(passed)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Ease {
    #[default]
    Linear,
    In,
    Out,
    InOut,
    OutQuad,
}

impl Ease {
    fn at(self, progress: f32) -> f32 {
        match self {
            Self::Linear => progress,
            Self::In => Curve::EASE_IN.ease(progress),
            Self::Out => Curve::EASE_OUT.ease(progress),
            Self::InOut => Curve::EASE_IN_OUT.ease(progress),
            Self::OutQuad => 1.0 - (1.0 - progress) * (1.0 - progress),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Wave {
    #[serde(deserialize_with = "finite")]
    cycles: f32,
    amplitude: Ramp,
}
