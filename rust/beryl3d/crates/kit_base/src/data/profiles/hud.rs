//! Layout choices and metrics used by the game's screen furniture.

use serde::Deserialize;

use super::super::tuning::{finite, fraction, nonnegative, positive};

/// Pieces of the screen drawn around the opening and the action bar.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Piece {
    Players,
    StepStops,
    FullControl,
    Phase,
    Menu,
    Gear,
    LogColumn,
    LogSheet,
    Viewer,
    Result,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Hud {
    pub table: Vec<Piece>,
    pub overlays: Vec<Piece>,
    pub table_under_opening: bool,
    pub anchored_to_window: bool,
    pub incoming: Incoming,
    /// Controls in this bar may move to keep the cards and phase icon clear.
    pub floating_prompt: Option<FloatingPrompt>,
    pub opening: Opening,
    pub result: ResultLayout,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Incoming {
    pub left: f32,
    pub top: f32,
    pub opponent_top: f32,
    #[serde(deserialize_with = "positive")]
    pub min_width: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_x: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_y: f32,
    #[serde(deserialize_with = "positive")]
    pub text: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FloatingPrompt {
    #[serde(deserialize_with = "positive")]
    pub button_min_width: f32,
    #[serde(deserialize_with = "positive")]
    pub character_width: f32,
    #[serde(deserialize_with = "positive")]
    pub button_text: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub button_padding: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub overlap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub asked_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub pill_text: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub pill_padding: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    pub start_choice: bool,
    pub waiting_dots: bool,
    pub wrapped_hand: Option<WrappedHand>,
    pub single_row: Option<SingleRow>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SingleRow {
    #[serde(deserialize_with = "positive")]
    pub coin_height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub coin_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub title_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub card: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub card_gap: f32,
    /// Degrees of rotation per step from the middle card.
    #[serde(deserialize_with = "nonnegative")]
    pub fan_turn: f32,
    /// Downward offset in stage points per squared step from the middle card.
    #[serde(deserialize_with = "nonnegative")]
    pub fan_drop: f32,
    #[serde(deserialize_with = "positive")]
    pub slot: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub slot_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub slots_gap: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WrappedHand {
    #[serde(deserialize_with = "positive")]
    pub coin: f32,
    #[serde(deserialize_with = "positive")]
    pub coin_narrow: f32,
    #[serde(deserialize_with = "positive")]
    pub narrow_below: f32,
    #[serde(deserialize_with = "positive")]
    pub coin_line: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub coin_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub title: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub title_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub card_most: f32,
    #[serde(deserialize_with = "positive")]
    pub card_most_narrow: f32,
    pub per_row: std::num::NonZeroUsize,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "fraction")]
    pub slot_scale: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub slots_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub slot_edge: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub no_actions: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub dock_bottom: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResultLayout {
    #[serde(deserialize_with = "positive")]
    pub title: f32,
    #[serde(deserialize_with = "positive")]
    pub reason: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub reason_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub card: f32,
    pub narrow_card: Option<NarrowCard>,
    #[serde(deserialize_with = "nonnegative")]
    pub card_gap: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NarrowCard {
    #[serde(deserialize_with = "positive")]
    pub below: f32,
    #[serde(deserialize_with = "positive")]
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Inspector {
    #[serde(deserialize_with = "positive")]
    pub stage_height: f32,
    #[serde(deserialize_with = "positive")]
    pub base_width: f32,
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "finite")]
    pub top: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub pad: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub artist: f32,
    #[serde(deserialize_with = "positive")]
    pub least_width: f32,
    #[serde(deserialize_with = "positive")]
    pub narrow_step: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub edge: f32,
    #[serde(deserialize_with = "finite")]
    pub orbs_right: f32,
    #[serde(deserialize_with = "finite")]
    pub between_top: f32,
    #[serde(deserialize_with = "finite")]
    pub stack_left: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub stack_gap: f32,
}
