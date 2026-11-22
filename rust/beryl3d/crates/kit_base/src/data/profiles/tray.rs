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
