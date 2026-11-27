//! The host-supplied layout profiles and the conditions that select them.

use std::collections::{BTreeMap, BTreeSet};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Deserializer};

use super::Card;
use super::tuning::{fraction, nonnegative, positive, read, vector};
use crate::geometry::{Insets, Size, Window};
mod screen;
pub use screen::{BugScreen, GalleryArrangement, ScreenMetrics, StartScreen};

pub mod anchored;
mod banded;
mod hud;
mod pile;
mod slot;
mod tray;
pub use anchored::Anchored;
pub use banded::Banded;
pub use hud::{
    FloatingPrompt, Heading, Hud, Incoming, Inspector, Piece, ResultLayout, SingleRow, WrappedHand,
};
pub use pile::PileGeometry;
pub use slot::SlotName;
pub use tray::{PutBack, TrayTuning};

/// The space a feature has for its content.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SizeClass {
    Compact,
    Regular,
}

impl SizeClass {
    /// The layout spelling used by existing inspection commands and story headers.
    #[must_use]
    pub const fn legacy_layout(self) -> &'static str {
        match self {
            Self::Regular => "desktop",
            Self::Compact => "phone",
        }
    }

    /// Reads the layout spelling used by existing inspection commands and story headers.
    #[must_use]
    pub fn from_legacy_layout(label: &str) -> Option<Self> {
        match label {
            "desktop" => Some(Self::Regular),
            "phone" => Some(Self::Compact),
            _ => None,
        }
    }
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
    /// The card's data (`card.json`): its printed size in millimetres, which a pose's scale of 1
    /// measures, and how far stacked cards lie apart. A profile does not write it:
    /// `Profiles::parse` copies it in, so every camera, and the placement that reads the camera,
    /// sizes cards as the card meshes do.
    #[serde(skip, default = "unset_card")]
    pub card: Card,
}

impl CameraData {
    fn validate(self) -> Result<(), String> {
        let [lower, upper] = self.required;
        let checks = [
            (
                (-90.0..90.0).contains(&self.pitch_degrees),
                "pitch_degrees",
                "must be at least -90 and less than 90 degrees",
            ),
            (
                self.fov_degrees > 0.0 && self.fov_degrees < 180.0,
                "fov_degrees",
                "must be greater than 0 and less than 180 degrees",
            ),
            (
                self.target.is_finite(),
                "target",
                "must have finite coordinates",
            ),
            (
                lower.is_finite() && upper.is_finite(),
                "required",
                "must have finite coordinates",
            ),
            (
                lower.x < upper.x && lower.y < upper.y,
                "required",
                "the lower corner must precede the upper corner on both axes",
            ),
        ];
        for (valid, field, why) in checks {
            if !valid {
                return Err(format!("table.camera.{field}: {why}"));
            }
        }
        Ok(())
    }
}

/// The card of a camera read from a profile, before `Profiles::parse` copies in the card's data.
fn unset_card() -> Card {
    Card {
        width: f32::NAN,
        height: f32::NAN,
        layer: f32::NAN,
        phased_out_opacity: f32::NAN,
    }
}

fn point<'de, D: Deserializer<'de>>(de: D) -> Result<Vec3, D::Error> {
    <[f32; 3]>::deserialize(de).map(Vec3::from_array)
}

fn corners<'de, D: Deserializer<'de>>(de: D) -> Result<[Vec2; 2], D::Error> {
    <[[f32; 2]; 2]>::deserialize(de).map(|pair| pair.map(Vec2::from_array))
}

/// The edges and depth of a table row in millimetres on the table.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub left: f32,
    pub right: f32,
    pub y: f32,
}

/// Dimensions for layouts that divide the window into player strips and a central play area.
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
#[serde(deny_unknown_fields)]
pub struct Table {
    pub camera: CameraData,
    pub creatures: Row,
    pub lands: Row,
    pub others: Row,
    pub bands: Option<Bands>,
    pub banded: Option<Banded>,
    pub anchored: Option<Anchored>,
    #[serde(deserialize_with = "positive")]
    pub strip: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub gap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub row_count_gap: f32,
    /// Centre-to-centre spacing for blockers of the same attacker, in card widths.
    #[serde(deserialize_with = "positive")]
    pub blocker_spacing: f32,
    pub cloth: Cloth,
    pub pile: PileGeometry,
    pub inspector: Option<Inspector>,
    #[serde(deserialize_with = "super::tuning::vector")]
    pub pile_step: Vec2,
    #[serde(deserialize_with = "vector")]
    pub beneath_step: Vec2,
    pub tray_tuning: TrayTuning,
    /// Dedicated tray geometry for ordering prompts.
    pub order_tray: Option<anchored::Tray>,
}

/// The square of cloth underneath the cards, in table millimetres.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Cloth {
    #[serde(deserialize_with = "positive")]
    pub size: f32,
    /// How far below the cards the cloth lies so that they never share a height.
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
}

/// Table presentation choices independent of container geometry.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the profile can configure these presentation choices independently"
)]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableStyle {
    pub show_optional_piles: bool,
    pub show_pile_sides: bool,
    pub show_copy_labels: bool,
    pub full_card_faces: bool,
    /// The distance below the opening hand's usual position, in points on the reference stage.
    #[serde(deserialize_with = "nonnegative")]
    pub hand_away: f32,
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
    /// Minimum diameter of the life orb's target outline, in logical points.
    pub mark_minimum: Option<f32>,
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
    PlayArea,
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
    #[serde(deserialize_with = "positive")]
    pub menu_preview_width: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub label_overlap: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub pool_right: f32,
    #[serde(deserialize_with = "positive")]
    pub pool_width: f32,
    #[serde(deserialize_with = "positive")]
    pub pool_done_width: f32,
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
    pub slots: BTreeMap<SlotName, SlotSpec>,
    /// Tuning for screens that use a scrolling column.
    pub screen: Option<ScreenMetrics>,
    pub gallery_arrangement: GalleryArrangement,
    pub manual: Option<ManualControls>,
    pub settings_quick_action_min_width: Option<f32>,
    test_matrix: Matrix,
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
                self.play.life.mark_minimum.is_none_or(is_positive),
                "play.life.mark_minimum",
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
        self.table.camera.validate()?;
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
        match (&self.table.anchored, &self.table.banded) {
            (Some(anchored), None) => {
                if anchored.tray.least > anchored.tray.most {
                    return Err("table.anchored.tray.least exceeds most".to_owned());
                }
                if anchored.stack.foot >= anchored.stack.top {
                    return Err("table.anchored.stack.foot must be below top".to_owned());
                }
            }
            (None, Some(banded)) => {
                if banded.tray.least > banded.tray.most {
                    return Err("table.banded.tray.least exceeds most".to_owned());
                }
            }
            _ => return Err("supply exactly one of table.anchored and table.banded".to_owned()),
        }
        if self
            .table
            .order_tray
            .is_some_and(|tray| tray.least > tray.most)
        {
            return Err("table.order_tray.least exceeds most".to_owned());
        }
        if let Some(inspector) = self.table.inspector
            && (inspector.least_width <= 2.0 * inspector.pad
                || inspector.least_width > inspector.width
                || inspector.width - inspector.narrow_step >= inspector.width)
        {
            return Err("invalid table.inspector width range or narrowing step".to_owned());
        }
        self.validate_slots()
    }

    fn validate_slots(&self) -> Result<(), String> {
        let compact = self.size_class == SizeClass::Compact;
        let floating_prompt = self.hud.floating_prompt.is_some();
        // Whether this profile must place each slot. An exhaustive match, so a new slot name
        // cannot be added without deciding when a profile needs it.
        let required = |name: SlotName| match name {
            SlotName::BugHint
            | SlotName::BugSheet
            | SlotName::BugTools
            | SlotName::ManualConnection
            | SlotName::ManualLog
            | SlotName::ManualMenu
            | SlotName::ManualNext
            | SlotName::ManualPool
            | SlotName::ManualProposals
            | SlotName::ScreenColumn
            | SlotName::ScreenDock => compact,
            SlotName::DivideEdge
            | SlotName::DivideSide
            | SlotName::ManualProposalsPanel
            | SlotName::ManualProposalsShort
            | SlotName::PromptCatalog
            | SlotName::PromptWays
            | SlotName::RevealedPile => false,
            SlotName::FullControl => self.hud.table.contains(&Piece::FullControl),
            SlotName::Gear => self.hud.table.contains(&Piece::Gear),
            SlotName::Inspector => self.table.inspector.is_some(),
            SlotName::LogToggle => self.hud.overlays.contains(&Piece::LogColumn),
            SlotName::Menu => self.hud.table.contains(&Piece::Menu),
            SlotName::Phase => self.hud.table.contains(&Piece::Phase),
            SlotName::PromptAsked
            | SlotName::PromptClearance
            | SlotName::PromptDock
            | SlotName::PromptStacked => floating_prompt,
            SlotName::Log
            | SlotName::Opening
            | SlotName::PlayerOpponent
            | SlotName::PlayerYou
            | SlotName::PromptBar
            | SlotName::Result
            | SlotName::ResultDock
            | SlotName::SettingsSheet
            | SlotName::Viewer => true,
        };
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
        for name in SlotName::ALL {
            if required(name) && !self.slots.contains_key(&name) {
                return Err(format!("missing slots.{name}"));
            }
        }
        if self.slots.contains_key(&SlotName::ManualMenu) && self.manual.is_none() {
            return Err("slots.manual.menu requires manual controls".to_owned());
        }
        if compact && self.screen.is_none() {
            return Err("missing screen metrics".to_owned());
        }
        let opening = self.hud.opening;
        if opening.single_row.is_some() == opening.wrapped_hand.is_some() {
            return Err("hud.opening requires exactly one hand layout".to_owned());
        }
        if opening.wrapped_hand.is_some() && self.screen.is_none() {
            return Err("hud.opening.wrapped_hand requires screen metrics".to_owned());
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
pub struct Profiles {
    automatic: Vec<ProfileData>,
    preview: Option<Box<ProfileData>>,
}

/// The profile files in selection order, relative to the host's data directory.
pub const FILES: [&str; 2] = ["profiles/phone.json", "profiles/desktop.json"];

impl Profiles {
    /// Loads the layout profiles in selection order.
    ///
    /// # Errors
    ///
    /// Returns the file and field at fault if the profiles are invalid.
    pub fn parse(files: &[(&str, &str)], card: Card) -> Result<Self, String> {
        let mut profiles = Vec::with_capacity(files.len());
        let mut previous = 0.0;
        let mut names = BTreeSet::new();
        for (index, &(file, text)) in files.iter().enumerate() {
            let mut profile: ProfileData = read(file, text)?;
            profile.table.camera.card = card;
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
        let profiles = Self {
            automatic: profiles,
            preview: None,
        };
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

    /// Selects a profile explicitly for all windows in this data set.
    ///
    /// # Errors
    ///
    /// Returns the file and field at fault if the preview profile is invalid or
    /// specifies a condition for automatic selection.
    pub fn with_preview(mut self, file: &str, text: &str, card: Card) -> Result<Self, String> {
        let mut profile: ProfileData = read(file, text)?;
        profile.table.camera.card = card;
        profile.validate().map_err(|why| format!("{file}: {why}"))?;
        if profile.shorter_side_below.is_some() {
            return Err(format!(
                "{file}: an explicit profile cannot set shorter_side_below"
            ));
        }
        self.preview = Some(Box::new(profile));
        Ok(self)
    }

    /// Selects an existing automatic profile for an authored window.
    #[must_use]
    pub fn selecting(&self, name: &str) -> Option<Self> {
        let profile = self.iter().find(|profile| profile.name == name)?;
        let mut profiles = self.clone();
        profiles.preview = Some(Box::new(profile.clone()));
        Some(profiles)
    }

    /// The explicit preview, the first matching automatic profile, or the fallback.
    #[must_use]
    pub fn for_size(&self, size: Size) -> &ProfileData {
        self.preview
            .as_deref()
            .or_else(|| {
                self.iter().find(|profile| {
                    profile
                        .shorter_side_below
                        .is_none_or(|limit| shorter_side_below(size, limit))
                })
            })
            .expect("validated profiles have a fallback")
    }

    /// The first automatic profile for a size class, used when a caller chooses only a class.
    ///
    /// # Panics
    ///
    /// If the class has no automatic profile. Loading profiles requires one for each class.
    #[must_use]
    pub fn for_size_class(&self, class: SizeClass) -> &ProfileData {
        self.iter()
            .find(|profile| profile.size_class == class)
            .expect("validated profiles cover every size class")
    }

    /// Selects a size class for the window passed to existing layout consumers.
    #[must_use]
    pub fn resolve_window(&self, mut window: Window) -> Window {
        window.size_class = Some(self.for_size(window.size).size_class);
        window
    }

    /// The final profile, which applies when no size limit does.
    #[must_use]
    pub fn fallback(&self) -> &ProfileData {
        self.automatic
            .last()
            .expect("validated profiles have a fallback")
    }

    /// The profiles eligible for automatic selection.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ProfileData> + DoubleEndedIterator {
        self.automatic.iter()
    }

    /// The profile explicitly selected by the host, when present.
    #[must_use]
    pub fn preview(&self) -> Option<&ProfileData> {
        self.preview.as_deref()
    }
}

/// The shorter dimension determines the layout even when the window is turned sideways.
fn shorter_side_below(size: Size, limit: f32) -> bool {
    size.width.min(size.height) < limit
}
