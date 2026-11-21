//! Positions and extents of the numbered gallery panels and their samples.

use glam::Vec2;
use serde::{Deserialize, Deserializer};

use crate::data::tuning::{nonnegative, nonzero_count, positive, positive_fraction, vector};

/// Boxes on the reference gallery page, in logical points. Sample positions are relative
/// to the inside of their panel's border.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GalleryLayout {
    #[serde(deserialize_with = "extent")]
    pub page: Vec2,
    #[serde(deserialize_with = "area")]
    pub wordmark: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub subtitle: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub wordmark_rules: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub wordmark_rule: Vec2,
    #[serde(deserialize_with = "positions")]
    pub panels_at: [Vec2; 9],
    #[serde(deserialize_with = "extents")]
    pub panels_size: [Vec2; 9],
    #[serde(deserialize_with = "nonnegative")]
    pub title_inset: f32,
    #[serde(deserialize_with = "positive")]
    pub title_height: f32,
    #[serde(deserialize_with = "vector")]
    pub title_padding: Vec2,
    #[serde(deserialize_with = "area")]
    pub title_number: [Vec2; 2],
    #[serde(deserialize_with = "nonnegative")]
    pub second_title_pull: f32,
    #[serde(deserialize_with = "extent")]
    pub caption: Vec2,
    #[serde(deserialize_with = "area")]
    pub chip: [Vec2; 2],
    #[serde(deserialize_with = "positive")]
    pub chip_step: f32,
    #[serde(deserialize_with = "positions")]
    pub swatch_labels: [Vec2; 5],
    #[serde(deserialize_with = "extent")]
    pub swatch_name: Vec2,
    #[serde(deserialize_with = "positive")]
    pub swatch_line: f32,
    #[serde(deserialize_with = "extent")]
    pub swatch_hex: Vec2,
    #[serde(deserialize_with = "area")]
    pub serif_specimen: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub serif_caption: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub specimen_divider: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub sans_specimen: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub sans_caption: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub tabletop: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub tabletop_caption: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub metal_rules: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub metal_rule: Vec2,
    #[serde(deserialize_with = "area")]
    pub metal_caption: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub done: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub view: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub pass: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub row_captions: [Vec2; 3],
    #[serde(deserialize_with = "extents")]
    pub row_caption_sizes: [Vec2; 3],
    #[serde(deserialize_with = "positions")]
    pub actions_rules: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub actions_rule: Vec2,
    #[serde(deserialize_with = "area")]
    pub pill: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub pill_text: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub tracker: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub phase: [Vec2; 2],
    #[serde(deserialize_with = "positive")]
    pub phase_step: f32,
    #[serde(deserialize_with = "positions")]
    pub orbs: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub orb: Vec2,
    #[serde(deserialize_with = "positions")]
    pub lives: [Vec2; 2],
    #[serde(deserialize_with = "extents")]
    pub life_sizes: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub orb_captions: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub zones_divider: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub badge: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub badge_text: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub zones_rule: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub library: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub library_card: Vec2,
    #[serde(deserialize_with = "vector")]
    pub library_depth: Vec2,
    #[serde(deserialize_with = "area")]
    pub graveyard: [Vec2; 2],
    #[serde(deserialize_with = "extent")]
    pub graveyard_card: Vec2,
    #[serde(deserialize_with = "vector")]
    pub graveyard_depth: Vec2,
    #[serde(deserialize_with = "positions")]
    pub lands: [Vec2; 3],
    #[serde(deserialize_with = "extent")]
    pub land: Vec2,
    #[serde(deserialize_with = "area")]
    pub land_count: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub pile_captions: [Vec2; 3],
    #[serde(deserialize_with = "vector")]
    pub face_inset: Vec2,
    #[serde(deserialize_with = "extent")]
    pub face_scale: Vec2,
    #[serde(deserialize_with = "positive_fraction")]
    pub face_radius: f32,
    #[serde(deserialize_with = "positions")]
    pub states_cards: [Vec2; 3],
    #[serde(deserialize_with = "positive")]
    pub states_card: f32,
    #[serde(deserialize_with = "area")]
    pub tapped: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub untapped: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub states_captions: [Vec2; 4],
    #[serde(deserialize_with = "positions")]
    pub hand: [Vec2; 3],
    #[serde(deserialize_with = "lengths")]
    pub hand_widths: [f32; 3],
    #[serde(deserialize_with = "positions")]
    pub combatants: [Vec2; 2],
    #[serde(deserialize_with = "positive")]
    pub combatant: f32,
    #[serde(deserialize_with = "positions")]
    pub combat_captions: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub combat_rule: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub stack_cards: [Vec2; 2],
    #[serde(deserialize_with = "positive")]
    pub stack_card: f32,
    #[serde(deserialize_with = "area")]
    pub ability: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_backdrop: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_title: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_instruction: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_zone: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub scry_backs: [Vec2; 2],
    #[serde(deserialize_with = "extents")]
    pub scry_back_sizes: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_top: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_bottom: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub scry_labels: [Vec2; 2],
    #[serde(deserialize_with = "lengths")]
    pub scry_label_widths: [f32; 2],
    #[serde(deserialize_with = "positive")]
    pub scry_label_height: f32,
    #[serde(deserialize_with = "area")]
    pub scry_done: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_view: [Vec2; 2],
    #[serde(deserialize_with = "area")]
    pub scry_source: [Vec2; 2],
    #[serde(deserialize_with = "positions")]
    pub scry_captions: [Vec2; 4],
    #[serde(deserialize_with = "extents")]
    pub scry_caption_widths: [Vec2; 2],
    #[serde(deserialize_with = "lengths")]
    pub prompt_columns: [f32; 3],
    #[serde(deserialize_with = "lengths")]
    pub prompt_rows: [f32; 3],
    #[serde(deserialize_with = "extent")]
    pub dock: Vec2,
    #[serde(deserialize_with = "extent")]
    pub stepper: Vec2,
    #[serde(deserialize_with = "positive")]
    pub stepper_field: f32,
    #[serde(deserialize_with = "positions")]
    pub menu_buttons: [Vec2; 3],
    #[serde(deserialize_with = "positions")]
    pub command_buttons: [Vec2; 3],
    #[serde(deserialize_with = "positive")]
    pub corner_width: f32,
    #[serde(deserialize_with = "nonzero_count")]
    pub library_layers: usize,
    #[serde(deserialize_with = "nonzero_count")]
    pub graveyard_layers: usize,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct Position(#[serde(deserialize_with = "vector")] Vec2);

#[derive(Deserialize)]
#[serde(transparent)]
struct Extent(#[serde(deserialize_with = "extent")] Vec2);

#[derive(Deserialize)]
#[serde(transparent)]
struct Length(#[serde(deserialize_with = "positive")] f32);

fn positions<'de, D: Deserializer<'de>, const N: usize>(de: D) -> Result<[Vec2; N], D::Error>
where
    [Position; N]: Deserialize<'de>,
{
    Ok(<[Position; N]>::deserialize(de)?.map(|position| position.0))
}

fn extents<'de, D: Deserializer<'de>, const N: usize>(de: D) -> Result<[Vec2; N], D::Error>
where
    [Extent; N]: Deserialize<'de>,
{
    Ok(<[Extent; N]>::deserialize(de)?.map(|extent| extent.0))
}

fn lengths<'de, D: Deserializer<'de>, const N: usize>(de: D) -> Result<[f32; N], D::Error>
where
    [Length; N]: Deserialize<'de>,
{
    Ok(<[Length; N]>::deserialize(de)?.map(|length| length.0))
}

fn extent<'de, D: Deserializer<'de>>(de: D) -> Result<Vec2, D::Error> {
    lengths(de).map(Vec2::from_array)
}

fn area<'de, D: Deserializer<'de>>(de: D) -> Result<[Vec2; 2], D::Error> {
    let (Position(point), Extent(size)) = Deserialize::deserialize(de)?;
    Ok([point, size])
}
