//! Table containers positioned against fixed anchors on the window or its reference stage.

use glam::Vec2;
use serde::Deserialize;

use super::super::tuning::{finite, nonnegative, positive, positive_fraction, vector};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Anchored {
    pub piles: Piles,
    pub shared_piles: SharedPiles,
    pub tray: Tray,
    pub hand: Hand,
    pub opponent_hand: Hand,
    pub stack: Stack,
    /// The gap from the back row to the life orb and step stops, in logical points.
    #[serde(deserialize_with = "nonnegative")]
    pub life_clearance: f32,
    /// The player anchors in normalized device coordinates, the viewer's first.
    pub players: [Player; 2],
    /// The smallest fraction of its full size a table row shrinks its cards to before they
    /// overlap. A crowded row shrinks only as far as it needs to fit its room with a gap
    /// between neighbours, as Arena's table does.
    #[serde(deserialize_with = "positive_fraction")]
    pub least_row_scale: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Piles {
    pub you: Pile,
    pub opponent: Pile,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pile {
    #[serde(deserialize_with = "vector")]
    pub outer: Vec2,
    #[serde(deserialize_with = "vector")]
    pub middle: Vec2,
    #[serde(deserialize_with = "vector")]
    pub inner: Vec2,
    #[serde(deserialize_with = "positive")]
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SharedPiles {
    /// Each shared pile's centre in normalized device coordinates. The three stand in one
    /// column at the table's left edge: the graveyard above the seam between the players'
    /// creature rows, the library on it and the exile pile below it.
    #[serde(deserialize_with = "vector")]
    pub library: Vec2,
    #[serde(deserialize_with = "vector")]
    pub graveyard: Vec2,
    #[serde(deserialize_with = "vector")]
    pub exile: Vec2,
    #[serde(deserialize_with = "positive")]
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tray {
    #[serde(deserialize_with = "positive")]
    pub stage_height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub y: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "positive")]
    pub half_most: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub stack_side: f32,
    #[serde(deserialize_with = "positive")]
    pub most: f32,
    #[serde(deserialize_with = "positive")]
    pub least: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub heading: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hand {
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_depth: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub height: f32,
    #[serde(deserialize_with = "finite")]
    pub edge: f32,
    #[serde(deserialize_with = "positive")]
    pub fan_radius: f32,
    #[serde(deserialize_with = "positive")]
    pub spacing: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub room: f32,
    #[serde(deserialize_with = "finite")]
    pub bias_degrees: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_depth: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub right: f32,
    #[serde(deserialize_with = "finite")]
    pub foot: f32,
    #[serde(deserialize_with = "finite")]
    pub top: f32,
    #[serde(deserialize_with = "positive")]
    pub step: f32,
    #[serde(deserialize_with = "positive")]
    pub reach: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lean: f32,
    #[serde(deserialize_with = "positive")]
    pub spare: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub least_scale: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Player {
    #[serde(deserialize_with = "vector")]
    pub life: Vec2,
    #[serde(deserialize_with = "vector")]
    pub pool: Vec2,
}
