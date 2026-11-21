//! Parsing and validation of the client's tuning files.

use glam::Vec2;
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Deserializer};

use super::light::{Light, Own};
use super::lighting::Lighting;
use super::playback::{CatchUp, FrameRate, Slide, TierProbe};
use super::pointer::PointerTuning;
use super::presentation::{Gallery, Notices, Revealed, Shown};
use super::{BadgeTuning, Card, effect::Effects, profiles::Profiles, verbs::Verbs};

/// The tuning data supplied by a host. The client uses the typed values directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    /// A card's printed size, which every card mesh, hit box and placement uses.
    pub card: Card,
    pub badges: BadgeTuning,
    pub verbs: Verbs,
    pub audit: Audit,
    pub pointer: PointerTuning,
    pub light: Light,
    pub own: Own,
    pub catch_up: CatchUp,
    pub frame_rate: FrameRate,
    pub slide: Slide,
    pub tier_probe: TierProbe,
    pub notices: Notices,
    pub revealed: Revealed,
    pub shown: Shown,
    pub gallery: Gallery,
    pub lighting: Lighting,
    pub profiles: Profiles,
    pub effects: Effects,
}

impl Data {
    /// Parses the card's size, badge sizes, spatial tuning, interface thresholds, layout profiles
    /// and effect definitions.
    /// The host supplies each tuning file through `load`. Errors name the input file and key.
    pub fn parse<'a>(
        mut load: impl FnMut(&str) -> Result<String, String>,
        profiles: &[(&str, &str)],
        effects: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, String> {
        let VerbFile { verbs } = read("verbs.json", &load("verbs.json")?)?;
        validate(&verbs).map_err(|(key, why)| format!("verbs.json: verbs.{key}: {why}"))?;
        let light = Light::parse(&load("light.json")?)?;
        let effects = Effects::parse(effects)?.with_lift(light.lift);
        for required in ["glow", "dissolve", "loss"] {
            if effects.get(required).is_none() {
                return Err(format!("effects/{required}.json: missing definition"));
            }
        }
        let card = Card::parse(&load("card.json")?)?;
        Ok(Self {
            card,
            badges: BadgeTuning::parse(&load("badges.json")?)?,
            verbs,
            audit: read("audit.json", &load("audit.json")?)?,
            pointer: PointerTuning::parse(&load("pointer.json")?)?,
            light,
            own: Own::parse(&load("own.json")?)?,
            catch_up: CatchUp::parse(&load("catch-up.json")?)?,
            frame_rate: FrameRate::parse(&load("frame-rate.json")?)?,
            slide: read("slide.json", &load("slide.json")?)?,
            tier_probe: read("tier-probe.json", &load("tier-probe.json")?)?,
            notices: read("notices.json", &load("notices.json")?)?,
            revealed: read("revealed.json", &load("revealed.json")?)?,
            shown: read("shown.json", &load("shown.json")?)?,
            gallery: read("gallery.json", &load("gallery.json")?)?,
            lighting: Lighting::parse(&load("lighting.json")?)?,
            profiles: Profiles::parse(profiles, card)?,
            effects,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerbFile {
    verbs: Verbs,
}

/// Thresholds shared by interface checks.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Audit {
    #[serde(deserialize_with = "nonnegative")]
    pub line_end_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub line_reshape_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub mark_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub anchor_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_ms: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step_points: f32,
    #[serde(deserialize_with = "fraction")]
    pub seen: f32,
    #[serde(deserialize_with = "fraction")]
    pub transparent: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub crowded: f32,
}

pub(super) fn read<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, String> {
    let mut json = serde_json::Deserializer::from_str(text);
    let data = serde_path_to_error::deserialize(&mut json).map_err(|error| {
        let path = error.path().to_string();
        let message = error.inner().to_string();
        let field = message
            .strip_prefix("missing field `")
            .and_then(|tail| tail.split_once('`'))
            .map_or("", |(field, _)| field);
        let key = match (path.as_str(), field) {
            (".", key) | (key, "") => key.to_owned(),
            (parent, field) => format!("{parent}.{field}"),
        };
        if key.is_empty() || key == "." {
            format!("{file}: {message}")
        } else {
            format!("{file}: {key}: {message}")
        }
    })?;
    json.end().map_err(|error| format!("{file}: {error}"))?;
    Ok(data)
}

fn validate(verbs: &Verbs) -> Result<(), (&'static str, &'static str)> {
    let fly = &verbs.fly;
    if fly.held.r#in + fly.held.hold >= 1.0 {
        return Err(("fly.held.hold", "flight and hold must leave time to land"));
    }
    for (path, turn) in [
        ("fly.turn.cast.each", fly.turn.cast),
        ("fly.turn.draw.each", fly.turn.draw),
        ("fly.held.turn.each", fly.held.turn),
    ] {
        if turn.each > turn.at.min(1.0 - turn.at) {
            return Err((path, "the turn must fit inside the flight"));
        }
    }
    if fly.thrown.risen > fly.thrown.held {
        return Err(("fly.thrown.held", "must be at or after the rise"));
    }
    if verbs.flip.grown >= verbs.flip.settle {
        return Err(("flip.settle", "must be after the growth"));
    }
    if verbs.streak.units < 1.0 {
        return Err(("streak.units", "must be at least one unit"));
    }
    if verbs.streak.least > verbs.streak.most {
        return Err(("streak.most", "must be at least as large as the minimum"));
    }
    Ok(())
}

fn number<'de, D: Deserializer<'de>>(
    de: D,
    valid: impl FnOnce(f32) -> bool,
    expected: &str,
) -> Result<f32, D::Error> {
    let value = f32::deserialize(de)?;
    if value.is_finite() && valid(value) {
        return Ok(value);
    }
    Err(D::Error::custom(format!(
        "expected {expected}, got {value}"
    )))
}

pub(super) fn finite<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |_| true, "a finite number")
}

pub(super) fn nonnegative<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v >= 0.0, "a finite nonnegative number")
}

pub(super) fn positive<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v > 0.0, "a finite positive number")
}

pub(super) fn fraction<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| (0.0..=1.0).contains(&v), "a number from 0 to 1")
}

pub(super) fn before_end<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| (0.0..1.0).contains(&v), "a number from 0 up to 1")
}

pub(super) fn positive_fraction<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v > 0.0 && v <= 1.0, "a positive number up to 1")
}

pub(super) fn interior<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v > 0.0 && v < 1.0, "a number between 0 and 1")
}

pub(super) fn nonzero_count<'de, D: Deserializer<'de>>(de: D) -> Result<usize, D::Error> {
    let value = usize::deserialize(de)?;
    if value == 0 {
        return Err(D::Error::custom("expected a count of at least one"));
    }
    Ok(value)
}

pub(super) fn vector<'de, D: Deserializer<'de>>(de: D) -> Result<Vec2, D::Error> {
    let vector = Vec2::from_array(<[f32; 2]>::deserialize(de)?);
    if !vector.is_finite() {
        return Err(D::Error::custom("expected two finite components"));
    }
    Ok(vector)
}
