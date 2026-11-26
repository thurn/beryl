//! Container dimensions for a table divided into horizontal bands.

use glam::Vec3;
use serde::{Deserialize, Deserializer};

use super::super::tuning::{finite, nonnegative, positive, vector};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Banded {
    pub piles: Piles,
    pub shared_piles: SharedPiles,
    pub stack: Stack,
    pub hand: Hand,
    pub opponent_hand: OpponentHand,
    pub tray: Tray,
    #[serde(deserialize_with = "nonnegative")]
    pub dock_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub button_height: f32,
    /// The resource pool's offset from the adjacent score, in logical points.
    #[serde(default, deserialize_with = "vector")]
    pub pool_offset: glam::Vec2,
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
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    /// The centres of the three pile slots from the left edge of the safe area.
    #[serde(deserialize_with = "centres")]
    pub x: Vec3,
    /// The minimum distance below the top safe inset.
    #[serde(default, deserialize_with = "nonnegative")]
    pub below_safe: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SharedPiles {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    /// The left pile's centre from the left edge of the safe area, in logical points.
    #[serde(deserialize_with = "positive")]
    pub left: f32,
    /// The right pile's centre from the right edge of the safe area, in logical points.
    #[serde(deserialize_with = "positive")]
    pub right: f32,
    /// Vertical offset from the seam between the two middle rows, in logical points.
    #[serde(default, deserialize_with = "finite")]
    pub y_offset: f32,
}

fn centres<'de, D: Deserializer<'de>>(de: D) -> Result<Vec3, D::Error> {
    <[f32; 3]>::deserialize(de).map(Vec3::from_array)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    /// A clear right-hand column allows the full card size, outside the rows.
    #[serde(default, deserialize_with = "nonnegative")]
    pub side_width: f32,
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "positive")]
    pub step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub count_room: f32,
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_depth: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hand {
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_depth: f32,
    #[serde(deserialize_with = "positive")]
    pub fan_radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "finite")]
    pub bias_degrees: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpponentHand {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "positive")]
    pub step: f32,
    #[serde(deserialize_with = "finite")]
    pub rise: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub right: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub room: f32,
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_depth: f32,
    #[serde(deserialize_with = "positive")]
    pub fan_radius: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tray {
    #[serde(deserialize_with = "nonnegative")]
    pub inset: f32,
    #[serde(deserialize_with = "positive")]
    pub most: f32,
    #[serde(deserialize_with = "positive")]
    pub least: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub heading: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least_step: f32,
}
