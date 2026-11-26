//! Geometry shared by trays in both kinds of table layout.

use glam::Vec2;
use serde::Deserialize;

use super::super::tuning::{nonnegative, positive, vector};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrayTuning {
    /// Millimetres in front of the camera for the first card.
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    pub fan: Fan,
    pub put_back: PutBack,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fan {
    #[serde(deserialize_with = "nonnegative")]
    pub step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub turn: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub drop: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PutBack {
    #[serde(deserialize_with = "nonnegative")]
    pub cascade: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub between: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least_step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub turn: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub drop: f32,
    #[serde(deserialize_with = "vector")]
    pub column_turns: Vec2,
    #[serde(deserialize_with = "nonnegative")]
    pub pile_rise: f32,
    pub deck_backs: usize,
    #[serde(deserialize_with = "vector")]
    pub deck_offset: Vec2,
}

impl PutBack {
    /// How many `deck_offset` steps the back `index` of the `deck_backs` lies from the front
    /// one, the deepest first: it also lies that many millimetres further from the eye.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a deck is drawn with a few backs"
    )]
    pub fn steps_under(&self, index: usize) -> f32 {
        self.deck_backs.saturating_sub(1 + index) as f32
    }

    /// How far above the front back the deepest one reaches, in stage pixels: the edge of the
    /// deck the backs draw, which a heading over it must clear.
    #[must_use]
    pub fn backs_edge(&self) -> f32 {
        (-self.deck_offset.y * self.steps_under(0)).max(0.0)
    }
}
