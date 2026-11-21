//! The host-supplied layout profiles and the conditions that select them.

use std::collections::{BTreeMap, BTreeSet};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer};

use super::tuning::{fraction, nonnegative, positive, read, vector};
use crate::geometry::{Insets, Size, Window};
mod screen;
pub use screen::{BugScreen, ScreenMetrics, StartScreen};

mod banded;
mod hud;
pub use banded::Banded;
pub use hud::{FloatingPrompt, Hud, Incoming, Inspector, Piece, ResultLayout, WrappedHand};

/// The space a feature has for its content.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    Compact,
    Regular,
}

/// A layout's camera, as data.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraData {
    /// The angle between the line of sight and straight down, in degrees.
    pub pitch_degrees: f32,
    /// The vertical field of view, in degrees.
    pub fov_degrees: f32,
    /// The point of the table the camera looks at, in millimetres.
    #[serde(deserialize_with = "point")]
    pub target: Vec3,
    /// The lowest and highest corners of the required rectangle, in millimetres on the table.
    #[serde(deserialize_with = "corners")]
    pub required: [Vec2; 2],
}

fn point<'de, D: Deserializer<'de>>(de: D) -> Result<Vec3, D::Error> {
    <[f32; 3]>::deserialize(de).map(Vec3::from_array)
}

fn corners<'de, D: Deserializer<'de>>(de: D) -> Result<[Vec2; 2], D::Error> {
    <[[f32; 2]; 2]>::deserialize(de).map(|pair| pair.map(Vec2::from_array))
}

/// The edges and depth of a battlefield row in millimetres on the table.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub left: f32,
    pub right: f32,
    pub y: f32,
}

/// Dimensions for layouts that divide the window into player strips and a battlefield.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bands {
    #[serde(deserialize_with = "nonnegative")]
    pub side: f32,
    #[serde(deserialize_with = "positive")]
    pub opponent_strip: f32,
    #[serde(deserialize_with = "positive")]
    pub you_strip: f32,
    #[serde(deserialize_with = "positive")]
    pub hand_width: f32,
    #[serde(deserialize_with = "super::tuning::positive_fraction")]
    pub hand_share: f32,
    #[serde(deserialize_with = "super::tuning::positive_fraction")]
    pub hand_shown: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub hand_top: f32,
}

/// The camera and the regions and limits of the table's containers.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Table {
    pub camera: CameraData,
    pub creatures: Row,
    pub lands: Row,
    pub others: Row,
    pub bands: Option<Bands>,
    pub banded: Option<Banded>,
    #[serde(deserialize_with = "positive")]
    pub strip: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub row_count_gap: f32,
    pub inspector: Option<Inspector>,
    #[serde(deserialize_with = "super::tuning::vector")]
    pub pile_step: Vec2,
    /// The remaining container values, also emitted as constants for placement that has yet
    /// to move to the profile API.
    #[serde(flatten)]
    pub containers: BTreeMap<String, serde_json::Value>,
}

/// Table presentation choices independent of container geometry.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the profile can configure these presentation choices independently"
)]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableStyle {
    pub show_exile: bool,
    pub show_pile_sides: bool,
    pub show_copy_labels: bool,
    pub full_card_faces: bool,
    /// The distance below the opening hand's usual position, in points on the reference stage.
    #[serde(deserialize_with = "nonnegative")]
    pub hand_away: f32,
    pub far_shade: bool,
    /// Whether overlaps lift successive nonland permanents away from the viewer.
    pub tuck_rise: bool,
}

/// Where the draw verb holds a card up for the viewer to read.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HeldDraw {
    Fixed {
        #[serde(deserialize_with = "positive")]
        size: f32,
        #[serde(deserialize_with = "vector")]
        ndc: Vec2,
    },
    OverRest {
        #[serde(deserialize_with = "positive")]
        size: f32,
        #[serde(deserialize_with = "fraction")]
        foot: f32,
    },
}

/// The text style of a life total.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifeText {
    Title,
    Life,
}

impl LifeText {
    #[must_use]
    pub const fn style(self) -> crate::typography::TypeStyle {
        match self {
            Self::Title => crate::tokens::type_style::TITLE,
            Self::Life => crate::tokens::type_style::LIFE,
        }
    }
}

/// Where animated gains and losses begin relative to the life display.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifeChange {
    AroundTotal,
    BesideOrb,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Life {
    #[serde(deserialize_with = "positive")]
    pub orb_scale: f32,
    pub text: LifeText,
    pub opening_hides_viewer: bool,
    pub change: LifeChange,
}

/// The layout-dependent settings used by playback.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Play {
    pub held_draw: HeldDraw,
    pub life: Life,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    #[default]
    SafeArea,
    Window,
    PlayerYou,
    PlayerOpponent,
    OpponentStrip,
    YourStrip,
    Middle,
    LogArea,
    PromptArea,
    Catalog,
    Battlefield,
    AboveViewerRows,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// The portion of a region made available for a feature.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SlotSpec {
    #[serde(default)]
    pub region: Region,
    /// Top, right, bottom and left, in logical points.
    #[serde(default)]
    pub inset: [f32; 4],
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    #[serde(default)]
    pub align_x: Align,
    #[serde(default)]
    pub align_y: Align,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Checks {
    #[serde(deserialize_with = "positive")]
    pub target_points: f32,
    /// The smallest text size the profile checks, if it sets one.
    pub text_points: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Matrix {
    safe_area: [f32; 4],
    usual_window: usize,
    /// Width, height and scale.
    windows: Vec<[f32; 3]>,
}

/// Dimensions of manual table controls that replace rows in a compact layout.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManualControls {
    #[serde(deserialize_with = "nonnegative")]
    pub label_overlap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub mana_right: f32,
    #[serde(deserialize_with = "positive")]
    pub mana_width: f32,
    #[serde(deserialize_with = "positive")]
    pub mana_done_width: f32,
}

/// A layout profile before it is fitted to a window.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProfileData {
    pub name: String,
    pub size_class: SizeClass,
    shorter_side_below: Option<f32>,
    pub table: Table,
    pub table_style: TableStyle,
    pub play: Play,
    pub checks: Checks,
    pub hud: Hud,
    pub slots: BTreeMap<String, SlotSpec>,
    /// Tuning for screens that use a scrolling column.
    pub screen: Option<ScreenMetrics>,
    pub manual: Option<ManualControls>,
    pub settings_quick_action_min_width: Option<f32>,
    test_matrix: Matrix,
    // Until the placement ports consume these values, the build still reads their old paths.
    #[serde(rename = "legacy")]
    _legacy: BTreeMap<String, serde_json::Value>,
    #[serde(rename = "counts")]
    _counts: Vec<String>,
}

impl ProfileData {
    /// The safe insets used for the profile's test windows.
    #[must_use]
    pub fn test_safe_area(&self) -> Insets {
        let [top, right, bottom, left] = self.test_matrix.safe_area;
        Insets {
            top,
            right,
            bottom,
            left,
        }
    }

    /// The windows this profile is tested in, in the order declared in its data.
    pub fn test_windows(&self) -> impl ExactSizeIterator<Item = Window> + '_ {
        self.test_matrix
            .windows
            .iter()
            .map(|&points| self.test_window(points))
    }

    /// The window used when inspecting this profile without an explicit size.
    #[must_use]
    pub fn usual_window(&self) -> Window {
        self.test_window(self.test_matrix.windows[self.test_matrix.usual_window])
    }

    fn test_window(&self, [width, height, scale]: [f32; 3]) -> Window {
        Window {
            size: Size { width, height },
            scale,
            safe_area: self.test_safe_area(),
            size_class: Some(self.size_class),
        }
    }

    fn validate(&self) -> Result<(), String> {
        let checks = [
            (
                self.shorter_side_below.is_none_or(is_positive),
                "shorter_side_below",
            ),
            (
                self.checks.text_points.is_none_or(is_positive),
                "checks.text_points",
            ),
            (
                self.settings_quick_action_min_width.is_none_or(is_positive),
                "settings_quick_action_min_width",
            ),
            (
                self.test_matrix.usual_window < self.test_matrix.windows.len(),
                "test_matrix.usual_window",
            ),
            (
                self.test_matrix.safe_area.into_iter().all(is_nonnegative),
                "test_matrix.safe_area",
            ),
        ];
        for (valid, field) in checks {
            if !valid {
                return Err(format!("invalid {field}"));
            }
        }
        for (index, window) in self.test_windows().enumerate() {
            let inset = window.safe_area;
            if !is_positive(window.scale)
                || !is_positive(window.size.width - inset.left - inset.right)
                || !is_positive(window.size.height - inset.top - inset.bottom)
            {
                return Err(format!("invalid test_matrix.windows[{index}]"));
            }
        }
        let camera = self.table.camera;
        let [lower, upper] = camera.required;
        if !(-90.0..90.0).contains(&camera.pitch_degrees)
            || !(0.0..180.0).contains(&camera.fov_degrees)
            || camera.fov_degrees == 0.0
            || !camera.target.is_finite()
            || !lower.is_finite()
            || !upper.is_finite()
            || lower.x >= upper.x
            || lower.y >= upper.y
        {
            return Err("invalid table.camera geometry".to_owned());
        }
        for (name, row) in [
            ("creatures", self.table.creatures),
            ("lands", self.table.lands),
            ("others", self.table.others),
        ] {
            if !row.left.is_finite()
                || !row.right.is_finite()
                || !row.y.is_finite()
                || row.left >= row.right
            {
                return Err(format!("invalid table.{name} geometry"));
            }
        }
        if self.table.bands.is_some() != self.table.banded.is_some() {
            return Err("table.bands and table.banded must be supplied together".to_owned());
        }
        self.validate_slots()
    }

    fn validate_slots(&self) -> Result<(), String> {
        for (name, slot) in &self.slots {
            if !matches!(
                slot.region,
                Region::SafeArea | Region::Window | Region::PlayerYou | Region::PlayerOpponent
            ) && self.table.bands.is_none()
            {
                return Err(format!("slots.{name} requires table.bands"));
            }
            if !slot.inset.into_iter().all(is_nonnegative)
                || !slot.max_width.is_none_or(is_positive)
                || !slot.max_height.is_none_or(is_positive)
            {
                return Err(format!("invalid dimensions for slots.{name}"));
            }
        }
        for name in [
            "settings.sheet",
            "prompt.bar",
            "opening",
            "player.you",
            "player.opponent",
            "log",
            "result",
            "result.dock",
            "viewer",
        ] {
            if !self.slots.contains_key(name) {
                return Err(format!("missing slots.{name}"));
            }
        }
        if self.size_class == SizeClass::Compact {
            if self.screen.is_none() {
                return Err("missing screen metrics".to_owned());
            }
            if self.manual.is_none() {
                return Err("missing manual controls".to_owned());
            }
            for name in [
                "screen.column",
                "screen.dock",
                "result.dock",
                "gallery.column",
                "bug.hint",
                "bug.tools",
                "bug.sheet",
                "manual.menu",
                "manual.connection",
                "manual.log",
                "manual.proposals",
                "manual.next",
                "manual.mana",
            ] {
                if !self.slots.contains_key(name) {
                    return Err(format!("missing slots.{name}"));
                }
            }
        }
        if self.hud.opening.wrapped_hand.is_some() && self.screen.is_none() {
            return Err("hud.opening.wrapped_hand requires screen metrics".to_owned());
        }
        let conditional_slots = [
            (self.hud.floating_prompt.is_some(), "prompt.dock"),
            (self.hud.floating_prompt.is_some(), "prompt.stacked"),
            (self.hud.floating_prompt.is_some(), "prompt.asked"),
            (self.hud.floating_prompt.is_some(), "prompt.clearance"),
            (self.hud.table.contains(&Piece::Menu), "menu"),
            (self.hud.table.contains(&Piece::Gear), "gear"),
            (self.hud.table.contains(&Piece::Phase), "phase"),
            (self.hud.table.contains(&Piece::FullControl), "full_control"),
            (self.hud.overlays.contains(&Piece::LogColumn), "log.toggle"),
            (self.table.inspector.is_some(), "inspector"),
        ];
        for (used, name) in conditional_slots {
            if used && !self.slots.contains_key(name) {
                return Err(format!("missing slots.{name}"));
            }
        }
        Ok(())
    }
}

fn is_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn is_nonnegative(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

/// Profiles ordered from the smallest applicable window to the fallback.
#[derive(Clone, Debug, PartialEq)]
pub struct Profiles(Vec<ProfileData>);

/// The profile files in selection order, relative to the host's data directory.
pub const FILES: [&str; 2] = ["profiles/phone.json", "profiles/desktop.json"];

/// The text the native host embeds, in the same order as the checkout loader.
pub const SHIPPED: [(&str, &str); 2] = [
    (
        FILES[0],
        include_str!("../../../../../../../data/beryl3d/profiles/phone.json"),
    ),
    (
        FILES[1],
        include_str!("../../../../../../../data/beryl3d/profiles/desktop.json"),
    ),
];

impl Profiles {
    /// Loads the layout profiles in selection order.
    ///
    /// # Errors
    ///
    /// Returns the file and field at fault if the profiles are invalid.
    pub fn parse(files: &[(&str, &str)]) -> Result<Self, String> {
        let mut profiles = Vec::with_capacity(files.len());
        let mut previous = 0.0;
        let mut names = BTreeSet::new();
        for (index, &(file, text)) in files.iter().enumerate() {
            let profile: ProfileData = read(file, text)?;
            profile.validate().map_err(|why| format!("{file}: {why}"))?;
            let fallback = index + 1 == files.len();
            if profile.shorter_side_below.is_none() != fallback
                || profile
                    .shorter_side_below
                    .is_some_and(|limit| limit <= previous)
            {
                return Err(format!(
                    "{file}: profiles must have increasing size limits followed by one fallback"
                ));
            }
            if !names.insert(profile.name.clone()) {
                return Err(format!("{file}: duplicate profile name {}", profile.name));
            }
            previous = profile.shorter_side_below.unwrap_or(previous);
            profiles.push(profile);
        }
        if profiles.is_empty() {
            return Err("no layout profiles supplied".to_owned());
        }
        let profiles = Self(profiles);
        for class in [SizeClass::Compact, SizeClass::Regular] {
            if !profiles.iter().any(|profile| profile.size_class == class) {
                return Err(format!(
                    "layout profiles are missing the {class:?} size class"
                ));
            }
        }
        for (profile, (file, _)) in profiles.iter().zip(files) {
            if profile
                .test_windows()
                .any(|window| !std::ptr::eq(profile, profiles.for_size(window.size)))
            {
                return Err(format!(
                    "{file}: test_matrix contains a window that selects another profile"
                ));
            }
        }
        Ok(profiles)
    }

    /// The first profile whose window condition fits, or the fallback.
    #[must_use]
    pub fn for_size(&self, size: Size) -> &ProfileData {
        self.iter()
            .find(|profile| {
                profile
                    .shorter_side_below
                    .is_none_or(|limit| shorter_side_below(size, limit))
            })
            .expect("validated profiles have a fallback")
    }

    /// Selects a size class for the window passed to existing layout consumers.
    #[must_use]
    pub fn resolve_window(&self, mut window: Window) -> Window {
        window.size_class = Some(self.for_size(window.size).size_class);
        window
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ProfileData> {
        self.0.iter()
    }
}

/// The shorter dimension determines the layout even when the window is turned sideways.
pub(crate) fn shorter_side_below(size: Size, limit: f32) -> bool {
    size.width.min(size.height) < limit
}
