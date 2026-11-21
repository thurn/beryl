//! The reference stage dimensions for fitting the table to the web interface.

use serde::Deserialize;

use super::super::tuning::{finite, nonnegative, positive, positive_fraction};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WebFit {
    pub stage: StageSize,
    pub perspective: Perspective,
    pub room: Room,
    #[serde(deserialize_with = "positive")]
    pub creature_width: f32,
    #[serde(deserialize_with = "positive")]
    pub land_width: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub land_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub tapped_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub permanent_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub advance: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub zoom_least: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub back_row_pull: f32,
    pub you: SideRows,
    pub opponent: SideRows,
    pub piles: Piles,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StageSize {
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "positive")]
    pub base: f32,
    #[serde(deserialize_with = "positive")]
    pub least: f32,
    #[serde(deserialize_with = "positive")]
    pub most: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Perspective {
    #[serde(deserialize_with = "positive")]
    pub distance: f32,
    #[serde(deserialize_with = "finite")]
    pub near_y: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub depth_per_px: f32,
    #[serde(deserialize_with = "finite")]
    pub vanishing_y: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Room {
    #[serde(deserialize_with = "nonnegative")]
    pub start: f32,
    #[serde(deserialize_with = "positive")]
    pub end: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SideRows {
    #[serde(deserialize_with = "nonnegative")]
    pub back: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lands: f32,
    #[serde(deserialize_with = "positive")]
    pub lands_end: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub permanents: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub others_x: f32,
    #[serde(deserialize_with = "positive")]
    pub others_end: f32,
    #[serde(deserialize_with = "positive")]
    pub others_width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Piles {
    #[serde(deserialize_with = "nonnegative")]
    pub clearance: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub most_thickness: f32,
    pub you: NearPiles,
    pub opponent: FarPiles,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NearPiles {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    pub x: [f32; 3],
    #[serde(deserialize_with = "nonnegative")]
    pub y: f32,
    #[serde(deserialize_with = "finite")]
    pub base: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FarPiles {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    pub x: [f32; 3],
    #[serde(deserialize_with = "nonnegative")]
    pub y: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub spread_down: f32,
}
