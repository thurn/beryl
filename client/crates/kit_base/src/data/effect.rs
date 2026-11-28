//! Effect definitions decoded by the data store and sampled by the frame player.

mod curve;
pub use curve::Scalar;

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::Vec2;
use serde::Deserialize;

use super::tuning::read;

/// Effects in shared script timing whose definitions every installation needs.
pub const SHIPPED: &[&str] = include!(concat!(env!("OUT_DIR"), "/effect_names.rs"));

/// Definitions indexed by the names used in script timing. Hosts parse these before playback.
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    definitions: BTreeMap<Arc<str>, Arc<Definition>>,
    lift: f32,
    palette: EffectPalette,
}

/// Arena-matched color ramp palette (`data/client/effects/palette.json`) used when an effect
/// layer resolves `"tint": "subject_colors"` against a card's colors.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EffectPalette {
    pub white: [u8; 4],
    pub blue: [u8; 4],
    pub black: [u8; 4],
    pub red: [u8; 4],
    pub green: [u8; 4],
    pub colorless: [u8; 4],
    pub multicolor: [u8; 4],
}

impl Default for EffectPalette {
    fn default() -> Self {
        Self {
            white: [255, 246, 214, 255],
            blue: [64, 168, 255, 255],
            black: [154, 92, 214, 255],
            red: [255, 88, 46, 255],
            green: [74, 206, 98, 255],
            colorless: [206, 216, 228, 255],
            multicolor: [248, 198, 72, 255],
        }
    }
}

impl EffectPalette {
    /// Parses a palette JSON file (`data/client/effects/palette.json`).
    ///
    /// # Errors
    ///
    /// The JSON is malformed or missing any required color ramp entry.
    pub fn parse(text: &str) -> Result<Self, String> {
        read("effects/palette.json", text)
    }

    /// Resolves a subject's color bitmask to one RGBA tint: colorless when empty, the matching
    /// single-color ramp entry for one color, or multicolor gold for two or more colors.
    #[must_use]
    pub const fn resolve(&self, colors: SubjectColors) -> [u8; 4] {
        match colors.0.count_ones() {
            0 => self.colorless,
            1 => match colors.0 {
                SubjectColors::WHITE_BIT => self.white,
                SubjectColors::BLUE_BIT => self.blue,
                SubjectColors::BLACK_BIT => self.black,
                SubjectColors::RED_BIT => self.red,
                SubjectColors::GREEN_BIT => self.green,
                _ => self.colorless,
            },
            _ => self.multicolor,
        }
    }
}

/// Compact 5-bit mask of a subject's colors (W, U, B, R, G) carried through effect playback
/// without allocating.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubjectColors(u8);

impl SubjectColors {
    const WHITE_BIT: u8 = 1 << 0;
    const BLUE_BIT: u8 = 1 << 1;
    const BLACK_BIT: u8 = 1 << 2;
    const RED_BIT: u8 = 1 << 3;
    const GREEN_BIT: u8 = 1 << 4;

    pub const NONE: Self = Self(0);
    pub const WHITE: Self = Self(Self::WHITE_BIT);
    pub const BLUE: Self = Self(Self::BLUE_BIT);
    pub const BLACK: Self = Self(Self::BLACK_BIT);
    pub const RED: Self = Self(Self::RED_BIT);
    pub const GREEN: Self = Self(Self::GREEN_BIT);

    /// Combines two color masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Parses single-letter color codes (`"W"`, `"U"`, `"B"`, `"R"`, `"G"`). Unknown codes are
    /// ignored.
    #[must_use]
    pub fn from_codes<'a>(codes: impl IntoIterator<Item = &'a str>) -> Self {
        let mut mask = 0_u8;
        for code in codes {
            mask |= match code {
                "W" => Self::WHITE_BIT,
                "U" => Self::BLUE_BIT,
                "B" => Self::BLACK_BIT,
                "R" => Self::RED_BIT,
                "G" => Self::GREEN_BIT,
                _ => 0,
            };
        }
        Self(mask)
    }
}

/// Which procedural effect `Material::Vfx` draws, stored in `Draw::uv[0]`.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Vfx {
    /// Unblocked attacking chevron: crimson-ember plasma body with luminous V-rim.
    Chevron = 0,
    /// Blocked attacking chevron: amber-to-gold plasma body with warm ivory V-rim.
    ChevronBlocked = 1,
    /// Blocking shield dome: polar-swirling golden forcefield curtain and crest ridge.
    ShieldDome = 2,
    /// One tapered, mitred segment of a luminous curved or straight line ribbon.
    LineRibbon = 3,
    /// Faceted glowing spearhead seated at the end of a luminous line.
    LineHead = 4,
    /// Test-only soft expanding ring whose radius follows normalized progress.
    Probe = 5,
    /// Positive or standard counter added burst (`PS_Counter_Spawn_Sm_KH`).
    CounterAdd = 6,
    /// Harmful (-1/-1 style) counter added burst (`vfx_counter_spawn_neg`).
    CounterAddNeg = 7,
    /// Counter removed shatter burst (`vfx_counter_removal`).
    CounterRemove = 8,
    /// Shield counter added crest burst (`VFX_SNC_Shield_Counter_Add`).
    ShieldAdd = 9,
    /// Shield counter broken shatter (`VFX_SNC_Shield_Counter_Break`).
    ShieldRemove = 10,
    /// Permanent landing dust, collision flash, shockwave, and ground shadow (`vfx_card_dust_*`).
    Settle = 11,
    /// Looping rounded-outline highlight with scrolling flare panner and thin additive rim.
    HighlightShimmer = 12,
    /// One-shot outward-expanding highlight appear pop burst.
    HighlightPop = 13,
    /// Mythic rare permanent arrival glow, flare, smoldering residue, and rings (`VFX_ETB_Mythic_Tier2_LUT`).
    MythicArrival = 14,
    /// Mana leaving a tapped source: a swirling ring in the mana's color round a hot pip, a ring
    /// burst, white sparks and a dark under-glow (`vfx_ManaTap_<Color>_Birth`).
    ManaBirth = 15,
    /// Mana landing on what it pays for: a four-point light-ping flare, colored wisps, sparks
    /// and side flares (`vfx_manaStack_<color>_hit`).
    ManaHit = 16,
}

/// Visual palette and shimmer style for outline highlight VFX (`Vfx::HighlightShimmer` and
/// `Vfx::HighlightPop`), encoded into `Draw::uv[3]` (`style` in `vfx.wgsl`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HighlightStyle {
    /// Playable or pickable cyan-mint shimmer and appear pop.
    #[default]
    Eligible = 0,
    /// Picked orange-gold shimmer and appear pop.
    Selected = 1,
    /// Pointer hover cyan-blue shimmer and appear pop.
    Hover = 2,
}

/// Size and air/ground variant for permanent-landing dust (`Vfx::Settle`), encoded into
/// `Draw::uv[3]` (`style` in `vfx.wgsl`).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettleKind {
    /// Mana value up to 3, ground permanent (`vfx_card_dust_sm`).
    #[default]
    Small = 0,
    /// Mana value 4 or 5, ground permanent (`vfx_card_dust_med`).
    Medium = 1,
    /// Mana value 6 or more, ground permanent (`vfx_card_dust_lg`).
    Large = 2,
    /// Mana value up to 3, flying creature (`vfx_card_dust_flying_sm`).
    FlyingSmall = 3,
    /// Mana value 4 or 5, flying creature (`vfx_card_dust_flying_med`).
    FlyingMedium = 4,
    /// Mana value 6 or more, flying creature (`vfx_card_dust_flying_lg`).
    FlyingLarge = 5,
}

impl SettleKind {
    /// Chooses the landing dust size tier from `mana_value` (`<= 3`, `4..=5`, `6+`) and `flying`.
    #[must_use]
    pub const fn from_card(mana_value: Option<u32>, flying: bool) -> Self {
        match (mana_value, flying) {
            (Some(6..), false) => Self::Large,
            (Some(4..=5), false) => Self::Medium,
            (_, false) => Self::Small,
            (Some(6..), true) => Self::FlyingLarge,
            (Some(4..=5), true) => Self::FlyingMedium,
            (_, true) => Self::FlyingSmall,
        }
    }

    /// Quad scale multiplier relative to the base `settle` VFX layer size.
    #[must_use]
    pub const fn quad_scale(self) -> f32 {
        match self {
            Self::Small | Self::FlyingSmall => 1.0,
            Self::Medium | Self::FlyingMedium => 1.20,
            Self::Large | Self::FlyingLarge => 1.45,
        }
    }
}

/// Either a fixed RGBA tint or a dynamic tint source resolved from the subject at draw time.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Tint {
    Rgba([u8; 4]),
    Source(TintSource),
}

impl Tint {
    /// Resolves this tint against `palette` and the subject's `colors`.
    #[must_use]
    pub const fn resolve(self, palette: &EffectPalette, colors: SubjectColors) -> [u8; 4] {
        match self {
            Self::Rgba(rgba) => rgba,
            Self::Source(TintSource::SubjectColors) => palette.resolve(colors),
        }
    }
}

/// Dynamic tint source names accepted by effect layers.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TintSource {
    SubjectColors,
}

impl Effects {
    /// Parses pairs of filename stems and JSON texts.
    ///
    /// # Errors
    ///
    /// A file is invalid, its name differs from its filename stem, or a name is repeated.
    pub fn parse<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Self, String> {
        let mut definitions = BTreeMap::new();
        for (name, text) in files {
            let file = format!("effects/{name}.json");
            let definition: Definition = read(&file, text)?;
            if definition.name.as_ref() != name {
                return Err(format!("{file}: name must match the filename"));
            }
            if definition.layers.is_empty() {
                return Err(format!(
                    "{file}: layers must contain at least one primitive"
                ));
            }
            for (index, layer) in definition.layers.iter().enumerate() {
                if !(0.0..layer.end).contains(&layer.start) || layer.end > 1.0 {
                    return Err(format!("{file}: layers[{index}] must fit within 0 to 1"));
                }
                layer
                    .primitive
                    .validate_ramps()
                    .map_err(|why| format!("{file}: layers[{index}].{why}"))?;
            }
            if definitions
                .insert(Arc::clone(&definition.name), Arc::new(definition))
                .is_some()
            {
                return Err(format!("{file}: duplicate definition"));
            }
        }
        Ok(Self {
            definitions,
            lift: 0.0,
            palette: EffectPalette::default(),
        })
    }

    /// The loaded definitions, ordered by their filename stems.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Definition> {
        self.definitions.values().map(Arc::as_ref)
    }

    /// Resolves a playback name to a definition that runs can keep across frames.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<Definition>> {
        self.definitions.get(name)
    }

    /// Sets the height, in millimetres, at which light lies over the face of what it is drawn
    /// on (`light.lift` in `light.json`), so that light is in front of a card held up toward
    /// the camera as well as one lying on the table.
    #[must_use]
    pub fn with_lift(mut self, lift: f32) -> Self {
        self.lift = lift;
        self
    }

    /// The height at which light lies over a surface, in millimetres.
    #[must_use]
    pub fn lift(&self) -> f32 {
        self.lift
    }

    /// Sets the color-ramp palette used by `"tint": "subject_colors"` layers.
    #[must_use]
    pub fn with_palette(mut self, palette: EffectPalette) -> Self {
        self.palette = palette;
        self
    }

    /// The color-ramp palette for resolving `"subject_colors"` tints.
    #[must_use]
    pub fn palette(&self) -> &EffectPalette {
        &self.palette
    }
}

/// One effect on the anchor selected when the step begins it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: Arc<str>,
    pub target: Target,
    /// The effect can be tried in stories before it has shared script timing.
    #[serde(default)]
    pub preview: bool,
    pub layers: Vec<Layer>,
}

impl Definition {
    /// Whether any layer draws with the subject's surface and its resolved pose.
    #[must_use]
    pub fn on_card(&self) -> bool {
        self.layers.iter().any(|layer| match layer.primitive {
            Primitive::Surface { surface, .. } => !matches!(surface, Surface::TextTint { .. }),
            Primitive::Burst { on_surface, .. } => on_surface,
            _ => false,
        })
    }
}

/// Whether the script may use its step's subject as the default anchor.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Subject,
    Named,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Clock {
    #[default]
    Eased,
    Elapsed,
}

/// A primitive and the interval on the effect clock over which to sample it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Layer {
    #[serde(default)]
    pub start: f32,
    #[serde(default = "one")]
    pub end: f32,
    #[serde(default)]
    pub clock: Clock,
    #[serde(default)]
    pub enhanced_only: bool,
    #[serde(default)]
    pub baseline_only: bool,
    #[serde(default)]
    pub mythic_only: bool,
    #[serde(flatten)]
    pub primitive: Primitive,
}

// Keep the JSON tag, the drawable variants and their fieldless inventory together.
macro_rules! primitives {
    ($($variant:ident = $word:literal { $($fields:tt)* },)*) => {
        /// The primitives the effect player can draw. Values that vary across a layer are curves.
        #[derive(Clone, Debug, Deserialize, PartialEq)]
        #[serde(tag = "kind", deny_unknown_fields)]
        pub enum Primitive {
            $(#[serde(rename = $word)] $variant { $($fields)* },)*
        }

        /// A primitive's kind without its tuning.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum PrimitiveKind {
            $($variant,)*
        }

        impl PrimitiveKind {
            /// Every primitive the effect player accepts.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The tag used in effect data files.
            #[must_use]
            pub const fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)*
                }
            }
        }

        impl Primitive {
            /// The kind named by this layer's JSON tag.
            #[must_use]
            pub const fn kind(&self) -> PrimitiveKind {
                match self {
                    $(Self::$variant { .. } => PrimitiveKind::$variant,)*
                }
            }
        }
    };
}

primitives! {
    Sprite = "sprite" {
        shape: Shape,
        quad: Quad,
    },
    Ring = "ring" {
        quad: Quad,
    },
    Burst = "burst" {
        quad: Quad,
        /// Draw with the surface, using its resolved pose and nominal size.
        #[serde(default)]
        on_surface: bool,
    },
    Directed = "directed" {
        sprite: DirectedSprite,
    },
    Streak = "streak" {
        lookback_ms: u16,
        width: Scalar,
        minimum: Scalar,
        tint: [u8; 4],
        opacity: Scalar,
        record: Option<LightRecord>,
    },
    Particles = "particles" {
        emitter: Emitter,
    },
    Vortex = "vortex" {
        emitter: Vortex,
    },
    Surface = "surface" {
        surface: Surface,
        amount: Scalar,
        #[serde(default)]
        record: bool,
    },
    Shake = "shake" {
        x: Scalar,
        y: Scalar,
    },
    Number = "number" {
        origin: NumberOrigin,
        drift: Scalar,
        opacity: Scalar,
    },
    Vfx = "vfx" {
        vfx: Vfx,
        /// Quad width in card widths over the layer's local progress.
        size: Scalar,
        tint: Tint,
        /// How much the layer fades out, from 0 (whole) to 1 (gone).
        fade: Scalar,
        /// Optional offset in card widths in the subject's plane.
        #[serde(default, deserialize_with = "optional_vector")]
        offset: Option<Vec2>,
        /// Quad height-to-width ratio; defaults to a square quad (`1.0`).
        #[serde(default = "one", deserialize_with = "super::tuning::positive")]
        aspect: f32,
    },
}

impl Primitive {
    // Serde loses the inner error paths of flattened layers and untagged curves.
    fn validate_ramps(&self) -> Result<(), String> {
        match self {
            Self::Sprite { quad, .. } | Self::Ring { quad } | Self::Burst { quad, .. } => {
                validate_ramps(&[
                    ("quad.scale", &quad.scale),
                    ("quad.fade", &quad.fade),
                    ("quad.alpha", &quad.alpha),
                ])?;
                validate_record("quad.record", quad.record.as_ref())?;
                validate_record("quad.point_record", quad.point_record.as_ref())
            }
            Self::Directed { sprite } => {
                validate_ramps(&[
                    ("sprite.length", &sprite.length),
                    ("sprite.width", &sprite.width),
                    ("sprite.alpha", &sprite.alpha),
                    ("sprite.fade", &sprite.fade),
                ])?;
                validate_record("sprite.record", sprite.record.as_ref())
            }
            Self::Streak {
                width,
                minimum,
                opacity,
                record,
                ..
            } => {
                validate_ramps(&[("width", width), ("minimum", minimum), ("opacity", opacity)])?;
                validate_record("record", record.as_ref())
            }
            Self::Particles { emitter } => validate_ramps(&[
                ("emitter.sweep", &emitter.sweep),
                ("emitter.size", &emitter.size),
                ("emitter.fall", &emitter.fall),
                ("emitter.drift", &emitter.drift),
                ("emitter.travel", &emitter.travel),
            ]),
            Self::Vortex { emitter } => validate_ramps(&[
                ("emitter.birth", &emitter.birth),
                ("emitter.size", &emitter.size),
            ]),
            Self::Surface { amount, .. } => validate_ramps(&[("amount", amount)]),
            Self::Shake { x, y } => validate_ramps(&[("x", x), ("y", y)]),
            Self::Number { drift, opacity, .. } => {
                validate_ramps(&[("drift", drift), ("opacity", opacity)])
            }
            Self::Vfx { size, fade, .. } => validate_ramps(&[("size", size), ("fade", fade)]),
        }
    }
}

fn validate_ramps(curves: &[(&str, &Scalar)]) -> Result<(), String> {
    for (path, curve) in curves {
        if let Some(ramp) = curve.reversed_ramp() {
            return Err(format!("{path}{ramp}: ramp start must be at or before end"));
        }
    }
    Ok(())
}

fn validate_record(path: &str, record: Option<&LightRecord>) -> Result<(), String> {
    if let Some(record) = record {
        validate_ramps(&[("scale", &record.scale), ("opacity", &record.opacity)])
            .map_err(|why| format!("{path}.{why}"))?;
    }
    Ok(())
}

/// Sprite cells used by effect quads. The halo's empty center is sized to the subject.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Disc,
    Cone,
    Comet,
    Ribbon,
    Outline,
    Head,
    Wedge,
    Crescent,
    Tap,
    Moon,
    Plaque,
    Coin,
    Halo,
    WedgeHot,
}

/// An additive quad placed over the subject's surface, scaled relative to the subject's size.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Quad {
    pub scale: Scalar,
    /// A fixed color, or `"subject_colors"` for the color the run carries or its subject has.
    pub tint: Tint,
    /// How much the draw material fades the light, from 0 (whole) to 1 (gone).
    pub fade: Scalar,
    /// Multiplier on the tint's alpha, applied before the material fade.
    #[serde(default = "whole")]
    pub alpha: Scalar,
    /// The box and opacity used to compare the light to the web element that represents it.
    pub record: Option<LightRecord>,
    /// A different comparison box for a point anchor, if supplied.
    pub point_record: Option<LightRecord>,
}

/// A sprite that starts at a point on the subject and extends in a direction in its plane.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DirectedSprite {
    pub shape: Shape,
    /// The starting point measured in shares of the subject's half extents.
    #[serde(deserialize_with = "super::tuning::vector")]
    pub origin: Vec2,
    pub direction: Direction,
    /// Length in shares of the subject's half extent along the sprite.
    pub length: Scalar,
    /// Width in shares of the subject's full extent across the sprite.
    pub width: Scalar,
    pub tint: [u8; 4],
    pub alpha: Scalar,
    pub fade: Scalar,
    pub record: Option<LightRecord>,
}

/// Which way the sprite's own positive y axis points in the subject's plane.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Left,
    Down,
    Right,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LightRecord {
    pub scale: Scalar,
    pub opacity: Scalar,
}

/// Particles emitted from a sweep over rows of the subject's face. Size is sampled by seed,
/// sweep by row, fall and drift by layer time, and travel by particle age.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Emitter {
    pub count: u16,
    pub sweep: Scalar,
    #[serde(deserialize_with = "super::tuning::positive_fraction")]
    pub life: f32,
    pub size: Scalar,
    pub fall: Scalar,
    pub drift: Scalar,
    pub travel: Scalar,
    pub tint: [u8; 4],
}

/// Particles born across the subject's face that rotate and shrink toward its center.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Vortex {
    pub count: u16,
    /// Birth time sampled by seed.
    pub birth: Scalar,
    #[serde(deserialize_with = "super::tuning::positive_fraction")]
    pub life: f32,
    pub size: Scalar,
    #[serde(deserialize_with = "super::tuning::finite")]
    pub turns: f32,
    pub tint: [u8; 4],
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
// Empty struct variants make Serde reject extra keys on channels without parameters.
#[serde(tag = "channel", rename_all = "snake_case", deny_unknown_fields)]
pub enum Surface {
    Whiten {},
    Tint { color: [u8; 4] },
    TextTint { color: [u8; 4] },
    Dissolve {},
    Opacity {},
    PreviousFace {},
}

/// Which side of an anchor a prepared number starts on.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NumberOrigin {
    Above,
    Below,
}

fn one() -> f32 {
    1.0
}

fn whole() -> Scalar {
    Scalar::Constant(1.0)
}

fn optional_vector<'de, D>(deserializer: D) -> Result<Option<Vec2>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some([x, y]) = Option::<[f32; 2]>::deserialize(deserializer)? else {
        return Ok(None);
    };
    if !x.is_finite() || !y.is_finite() {
        return Err(serde::de::Error::custom("must be finite"));
    }
    Ok(Some(Vec2::new(x, y)))
}
