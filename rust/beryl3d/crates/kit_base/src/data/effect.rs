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
        })
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
}

/// One effect on the anchor selected when the step begins it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: Arc<str>,
    pub target: Target,
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
    #[serde(flatten)]
    pub primitive: Primitive,
}

/// The primitives the effect player can draw. Values that vary across a layer are curves.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Primitive {
    Sprite {
        shape: Shape,
        quad: Quad,
    },
    Ring {
        quad: Quad,
    },
    Burst {
        quad: Quad,
        /// Draw with the surface, using its resolved pose and nominal size.
        #[serde(default)]
        on_surface: bool,
    },
    Directed {
        sprite: DirectedSprite,
    },
    Streak {
        lookback_ms: u16,
        width: Scalar,
        minimum: Scalar,
        tint: [u8; 4],
        opacity: Scalar,
        record: Option<LightRecord>,
    },
    Particles {
        emitter: Emitter,
    },
    Vortex {
        emitter: Vortex,
    },
    Surface {
        surface: Surface,
        amount: Scalar,
        #[serde(default)]
        record: bool,
    },
    Shake {
        x: Scalar,
        y: Scalar,
    },
    Number {
        origin: NumberOrigin,
        drift: Scalar,
        opacity: Scalar,
    },
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
    pub tint: [u8; 4],
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
