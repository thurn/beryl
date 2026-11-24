//! The sizes of object badges, from
//! `data/client/badges.json`. Lengths are in the web card's em, a share of a table card's
//! width (`em`), unless a field says otherwise. Sizes in points are the sizes text is shaped at,
//! so a change to one reshapes the badges' text.

use serde::Deserialize;

use super::tuning::{nonnegative, positive, read};

/// Every badge's sizes.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BadgeTuning {
    /// The web card's em, as a share of a table card's width.
    #[serde(deserialize_with = "positive")]
    pub em: f32,
    /// How far a badge's panel lies above the card's face; its text lies twice as far.
    #[serde(deserialize_with = "nonnegative")]
    pub raise: f32,
    /// The size in points every badge's numbers and named counters' names are shaped at.
    #[serde(deserialize_with = "positive")]
    pub numeral_points: f32,
    pub plaque: Plaque,
    pub counter: CounterBadge,
    pub lock: LockBadge,
    pub count: CountBadge,
    pub note: NoteTag,
    pub copy: CopyTag,
}

impl BadgeTuning {
    /// Reads `badges.json`'s text.
    ///
    /// # Errors
    ///
    /// Returns the key at fault if the text is not valid badge tuning.
    pub fn parse(text: &str) -> Result<Self, String> {
        read("badges.json", text)
    }
}

/// The plaque of power and toughness, or loyalty, at the card's bottom right.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plaque {
    #[serde(deserialize_with = "nonnegative")]
    pub right: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub bottom: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub min_width: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub border: f32,
    #[serde(deserialize_with = "positive")]
    pub numerals: f32,
}

/// A counter's badge along the card's foot, and the named counters' rows above the plaque.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CounterBadge {
    #[serde(deserialize_with = "nonnegative")]
    pub left: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub right: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub bottom: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub border: f32,
    #[serde(deserialize_with = "positive")]
    pub numerals: f32,
    #[serde(deserialize_with = "positive")]
    pub name: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub name_gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub named_bottom: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub named_right: f32,
}

/// The lock of a permanent that doesn't untap, in the card's top left corner.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LockBadge {
    #[serde(deserialize_with = "nonnegative")]
    pub left: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub top: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "positive")]
    pub icon: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_left: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_right: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub border: f32,
}

/// How many identical permanents a pile holds, at the card's top right.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CountBadge {
    #[serde(deserialize_with = "nonnegative")]
    pub right: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub top: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub border: f32,
    #[serde(deserialize_with = "positive")]
    pub numerals: f32,
}

/// A permanent's note or chosen value, a line across the top of its face.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NoteTag {
    #[serde(deserialize_with = "nonnegative")]
    pub top: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub side: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_x: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_y: f32,
    #[serde(deserialize_with = "positive")]
    pub text: f32,
    #[serde(deserialize_with = "positive")]
    pub line_height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "positive")]
    pub points: f32,
}

/// A copy's label, at the left of its foot.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CopyTag {
    #[serde(deserialize_with = "nonnegative")]
    pub padding_x: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_y: f32,
    #[serde(deserialize_with = "positive")]
    pub text: f32,
    #[serde(deserialize_with = "positive")]
    pub line_height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub border: f32,
    #[serde(deserialize_with = "positive")]
    pub points: f32,
}
