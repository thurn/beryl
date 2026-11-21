//! Parsing and validation of the client's tuning files.

use glam::Vec2;
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Deserializer};

use super::profiles::Profiles;
use super::verbs::Verbs;

/// The tuning data supplied by a host. The client uses the typed values directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    pub verbs: Verbs,
    pub audit: Audit,
    pub profiles: Profiles,
}

impl Data {
    /// Parses the spatial tuning, the interface thresholds and the layout profiles.
    /// Errors name the input file and the path to the offending value.
    pub fn parse(space: &str, gates: &str, profiles: &[(&str, &str)]) -> Result<Self, String> {
        let Space { verbs } = read("space.json", space)?;
        let Gates { audit } = read("gates.json", gates)?;
        validate(&verbs).map_err(|(key, why)| format!("space.json: verbs.{key}: {why}"))?;
        Ok(Self {
            verbs,
            audit,
            profiles: Profiles::parse(profiles)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Space {
    verbs: Verbs,
}

// The renderer and the interface runner own the other sections of gates.json.
#[derive(Deserialize)]
struct Gates {
    audit: Audit,
}

/// Thresholds shared by interface checks.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Audit {
    #[serde(deserialize_with = "nonnegative")]
    pub line_end_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub line_reshape_points: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub mark_points: f32,
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

pub(super) fn columns<'de, D: Deserializer<'de>>(de: D) -> Result<usize, D::Error> {
    let value = usize::deserialize(de)?;
    if value == 0 {
        return Err(D::Error::custom("expected at least one column"));
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
