//! Container dimensions for a table divided into horizontal bands.

use glam::Vec3;
use serde::{Deserialize, Deserializer};

use super::super::tuning::{finite, nonnegative, positive, positive_fraction, vector};

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
    /// The point of the pool's own box that stands at `pool_offset`, as fractions of its
    /// width and height: `[0, 0]` is its top left corner, `[1, 0.5]` the middle of its
    /// right edge.
    #[serde(default, deserialize_with = "vector")]
    pub pool_align: glam::Vec2,
    /// How the battlefield's rows shrink, or `None` to keep their cards at full size and let a
    /// crowded row overlap and scroll.
    #[serde(default)]
    pub rows: Option<Rows>,
}

/// How a banded table's battlefield rows shrink to fit their cards.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rows {
    /// The narrowest a card of the creatures' and other permanents' rows is drawn, in logical
    /// points, so that each stays a target a finger can press. A row shrinks only as far as it
    /// needs to fit its room, and past this its cards overlap.
    #[serde(deserialize_with = "positive")]
    pub least_width: f32,
    /// The least scale of a crowded lands' row, as a fraction of a full-size card. Lands are no
    /// targets of their own, so their row may shrink further than the others.
    #[serde(deserialize_with = "positive_fraction")]
    pub land_least: f32,
    /// The widest a land is drawn, in logical points, so that the lands' rows leave the other
    /// permanents most of the battlefield. Lands this small are read by pressing them, and are not
    /// targets of their own for the touch-target checks (`Layout::too_small_to_press`).
    #[serde(deserialize_with = "positive")]
    pub land_width: f32,
    /// The table the lands' rows take beyond the camera's required rectangle on each side, in
    /// millimetres, where the cards of the other rows can stay `least_width` points wide with
    /// it; on a phone too short for that, the lands get less (`phone::fit_camera`).
    #[serde(deserialize_with = "positive")]
    pub land_room: f32,
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
    /// The piles' centre from the left edge of the safe area, in logical points. The three
    /// stand in one column there: the graveyard above the library and the exile pile below it.
    #[serde(deserialize_with = "positive")]
    pub left: f32,
    /// The space between two piles of the column, in logical points.
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
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
    /// The width of a column at the right of the battlefield, in logical points, in which the
    /// stack's items stand centred over the rows, or 0 to centre them across the middle band.
    /// Either way they are `width` wide unless that would leave a creature no strip to press.
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
    /// The room a stack centred in the middle band leaves free at each end of the band, in
    /// logical points. The HUD's phase icon stands at the band's right end, and the stack keeps
    /// the same room at its left end so that it stays centred.
    #[serde(default, deserialize_with = "nonnegative")]
    pub ends: f32,
    /// The same room on a table whose players share one library and graveyard, which lie at the
    /// band's two ends with the phase icon inside the graveyard.
    #[serde(default, deserialize_with = "nonnegative")]
    pub shared_ends: f32,
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
    /// Whether every row of the viewer's hand above the front row starts right of the viewer's
    /// piles and Mana button, so that a hand too large for one row leaves them in view. Without
    /// it the rows are centred and hold as nearly the same number of cards as they can, as the
    /// phone's do.
    #[serde(default)]
    pub clear_piles: bool,
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
