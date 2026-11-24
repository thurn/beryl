//! Screen arrangements and dimensions supplied by a layout profile.

use serde::Deserialize;

use crate::data::tuning::positive;

/// Where the story catalog sits relative to the preview.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GalleryArrangement {
    SideBySide,
    Stacked,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScreenMetrics {
    #[serde(deserialize_with = "positive")]
    pub action_height: f32,
    #[serde(deserialize_with = "positive")]
    pub action_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub action_text: f32,
    #[serde(deserialize_with = "positive")]
    pub list_row: f32,
    pub start: StartScreen,
    pub bug: BugScreen,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartScreen {
    #[serde(deserialize_with = "positive")]
    pub corner: f32,
    #[serde(deserialize_with = "positive")]
    pub gap: f32,
    #[serde(deserialize_with = "positive")]
    pub agent_row: f32,
    #[serde(deserialize_with = "positive")]
    pub agent_gap: f32,
    #[serde(deserialize_with = "positive")]
    pub agent_padding_y: f32,
    #[serde(deserialize_with = "positive")]
    pub agent_padding_x: f32,
    #[serde(deserialize_with = "positive")]
    pub radio: f32,
    #[serde(deserialize_with = "positive")]
    pub label: f32,
    #[serde(deserialize_with = "positive")]
    pub agent_name: f32,
    #[serde(deserialize_with = "positive")]
    pub about: f32,
    #[serde(deserialize_with = "positive")]
    pub agents_least: f32,
    #[serde(deserialize_with = "positive")]
    pub field: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BugScreen {
    #[serde(deserialize_with = "positive")]
    pub hint: f32,
    #[serde(deserialize_with = "positive")]
    pub padding_y: f32,
    #[serde(deserialize_with = "positive")]
    pub padding_x: f32,
    #[serde(deserialize_with = "positive")]
    pub radius: f32,
    #[serde(deserialize_with = "positive")]
    pub gap: f32,
    #[serde(deserialize_with = "positive")]
    pub title: f32,
    #[serde(deserialize_with = "positive")]
    pub description_least: f32,
    #[serde(deserialize_with = "positive")]
    pub description_most: f32,
    #[serde(deserialize_with = "positive")]
    pub submit_width: f32,
}
