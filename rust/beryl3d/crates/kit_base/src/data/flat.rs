//! Shared flat-layer and manual-table tuning.

use serde::Deserialize;

use super::tuning::{fraction, nonnegative, positive};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Flat {
    #[serde(deserialize_with = "nonnegative")]
    pub transition_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub menu_edge: f32,
    pub tooltip: Tooltip,
    pub manual: Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tooltip {
    pub delay_ms: u32,
    #[serde(deserialize_with = "nonnegative")]
    pub fade_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manual {
    #[serde(deserialize_with = "nonnegative")]
    pub edge: f32,
    pub palette: Palette,
    pub log: ManualLog,
    pub drop: DropTarget,
    pub link: LinkBanner,
    #[serde(deserialize_with = "nonnegative")]
    pub sweep_rim: f32,
    pub selection: Selection,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub top: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualLog {
    #[serde(deserialize_with = "positive")]
    pub width: f32,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DropTarget {
    #[serde(deserialize_with = "fraction")]
    pub opponent_hand: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub rim: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LinkBanner {
    #[serde(deserialize_with = "nonnegative")]
    pub top: f32,
    #[serde(deserialize_with = "fraction")]
    pub top_share: f32,
    #[serde(deserialize_with = "positive")]
    pub width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(deserialize_with = "nonnegative")]
    pub above: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_y: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub padding_x: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub radius: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub rim: f32,
}

impl Flat {
    /// The bundled tuning for standalone toolkit callers.
    #[must_use]
    pub fn shipped() -> Self {
        super::tuning::read(
            "flat.json",
            include_str!("../../../../../../data/beryl3d/flat.json"),
        )
        .expect("bundled flat-layer tuning")
    }
}

impl Tooltip {
    /// The wait before a resting pointer reveals a tooltip, on the frame clock.
    #[must_use]
    pub const fn delay_ns(self) -> u64 {
        self.delay_ms as u64 * 1_000_000
    }
}
