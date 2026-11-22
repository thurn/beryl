//! The positions of cards and exposed edges in zone piles.

use glam::Vec2;
use serde::{Deserialize, Deserializer};

use super::super::tuning::{fraction, nonnegative, positive, vector};

/// How cards stand in zone piles and where their exposed edges are drawn.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PileGeometry {
    /// The distance in millimetres from the pile's base at which its top card still counts as
    /// being on the pile.
    #[serde(deserialize_with = "nonnegative")]
    pub top_tolerance: f32,
    /// The height of a hidden pile above the table, in card layers.
    #[serde(deserialize_with = "fraction")]
    pub hidden_pile_layer: f32,
    /// The shaded edge's offset below its card, in card layers.
    #[serde(deserialize_with = "fraction")]
    pub rim_shade_layer: f32,
    /// Reference stage size in points for rim offsets when there is no projection fit.
    #[serde(deserialize_with = "stage_size")]
    pub rim_stage: Vec2,
    /// The card width at which one point of rim line is drawn at full size.
    #[serde(deserialize_with = "positive")]
    pub rim_card_width: f32,
    /// How many cards below the top receive the full rim spacing.
    pub rim_spread_cards: u8,
    /// The offset per card in the spread portion of the pile, in reference stage points.
    #[serde(deserialize_with = "nonnegative")]
    pub rim_step: f32,
    /// The additional offset the curved portion approaches, in reference stage points.
    #[serde(deserialize_with = "nonnegative")]
    pub rim_curve_span: f32,
    /// The number of cards below the top at which the curve reaches half its span.
    #[serde(deserialize_with = "positive")]
    pub rim_curve_depth: f32,
    /// The opponent's rim offset for each point the viewer's rim extends to the right.
    #[serde(deserialize_with = "vector")]
    pub opponent_rim_direction: Vec2,
}

fn stage_size<'de, D: Deserializer<'de>>(de: D) -> Result<Vec2, D::Error> {
    let stage = vector(de)?;
    if stage.min_element() <= 0.0 {
        return Err(serde::de::Error::custom("expected a positive stage size"));
    }
    Ok(stage)
}
