//! A card's printed size and how cards lie on one another, from `data/beryl3d/card.json`.

use glam::Vec2;
use serde::Deserialize;

use super::tuning::{fraction, nonnegative, positive, read};

/// A card's printed size in millimetres. A card mesh is this size, so a pose's scale of 1 draws a
/// card at its real size, and placement, hit testing and the renderer's card mesh all read it
/// from here.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Card {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    /// The height between two cards lying one on the other, in a pile or an overlapped row.
    #[serde(deserialize_with = "nonnegative")]
    pub layer: f32,
    /// The opacity of a phased-out permanent.
    #[serde(deserialize_with = "fraction")]
    pub phased_out_opacity: f32,
}

impl Card {
    /// Reads `card.json`'s text.
    ///
    /// # Errors
    ///
    /// Returns the key at fault if the text is not a valid card.
    pub fn parse(text: &str) -> Result<Self, String> {
        read("card.json", text)
    }

    /// The card's width and height.
    #[must_use]
    pub fn size(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// Half the card's width and height: the distance from its middle to its edges.
    #[must_use]
    pub fn half(self) -> Vec2 {
        self.size() / 2.0
    }

    /// The card's height for each unit of its width.
    #[must_use]
    pub fn height_per_width(self) -> f32 {
        self.height / self.width
    }
}
