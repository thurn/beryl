//! Parsing and validation of the client's tuning files.

use glam::{Quat, Vec2};
use serde::de::{DeserializeOwned, Error};
use serde::{Deserialize, Deserializer};

use super::effect::{Effects, SHIPPED};
use super::flat::Flat;
use super::hud::Hud;
use super::light::{Light, Own};
use super::lighting::Lighting;
use super::playback::{CatchUp, FrameRate, Slide, TierProbe};
use super::pointer::PointerTuning;
use super::presentation::{Notices, Revealed, Shown};
use super::{BadgeTuning, Card, profiles::Profiles, verbs::Verbs};

/// A runtime tuning input and the type that decodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuningFile {
    pub name: &'static str,
    pub reader: &'static str,
}

struct File<T> {
    name: &'static str,
    reader: std::marker::PhantomData<T>,
}

impl<T> File<T> {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            reader: std::marker::PhantomData,
        }
    }

    fn decode(
        &self,
        load: &mut impl FnMut(&str) -> Result<String, String>,
        parse: impl FnOnce(&str, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        parse(self.name, &load(self.name)?)
    }
}

// The typed file constants, watcher inputs and map entries share one declaration.
macro_rules! tuning_files {
    ($($id:ident: $reader:ident = $name:literal;)*) => {
        $(const $id: File<$reader> = File::new($name);)*
        /// Runtime tuning files at the root of the data directory.
        pub const TUNING_FILES: &[TuningFile] = &[
            $(TuningFile { name: $id.name, reader: stringify!($reader) },)*
        ];
        /// Names used by hosts that watch runtime tuning.
        pub const TUNING_FILE_NAMES: [&str; TUNING_FILES.len()] = [$($id.name,)*];
    };
}

tuning_files! {
    CARD: Card = "card.json";
    BADGES: BadgeTuning = "badges.json";
    VERBS: VerbFile = "verbs.json";
    AUDIT: Audit = "audit.json";
    FLAT: Flat = "flat.json";
    HUD: Hud = "hud.json";
    POINTER: PointerTuning = "pointer.json";
    LIGHT: Light = "light.json";
    OWN: Own = "own.json";
    CATCH_UP: CatchUp = "catch-up.json";
    FRAME_RATE: FrameRate = "frame-rate.json";
    SLIDE: Slide = "slide.json";
    TIER_PROBE: TierProbe = "tier-probe.json";
    NOTICES: Notices = "notices.json";
    REVEALED: Revealed = "revealed.json";
    SHOWN: Shown = "shown.json";
    LIGHTING: Lighting = "lighting.json";
}

/// The tuning data supplied by a host. The client uses the typed values directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    /// A card's printed size, which every card mesh, hit box and placement uses.
    pub card: Card,
    pub badges: BadgeTuning,
    pub verbs: Verbs,
    pub flat: super::flat::Flat,
    pub hud: super::hud::Hud,
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
        let VerbFile { verbs } = VERBS.decode(&mut load, read)?;
        validate(&verbs).map_err(|(key, why)| format!("verbs.json: verbs.{key}: {why}"))?;
        let light = LIGHT.decode(&mut load, |_, text| Light::parse(text))?;
        let effects = Effects::parse(effects)?.with_lift(light.lift);
        for &required in SHIPPED {
            if effects.get(required).is_none() {
                return Err(format!("effects/{required}.json: missing definition"));
            }
        }
        let card = CARD.decode(&mut load, |_, text| Card::parse(text))?;
        Ok(Self {
            card,
            badges: BADGES.decode(&mut load, |_, text| BadgeTuning::parse(text))?,
            verbs,
            flat: FLAT.decode(&mut load, read)?,
            hud: HUD.decode(&mut load, read)?,
            audit: AUDIT.decode(&mut load, read)?,
            pointer: POINTER.decode(&mut load, |_, text| PointerTuning::parse(text))?,
            light,
            own: OWN.decode(&mut load, |_, text| Own::parse(text))?,
            catch_up: CATCH_UP.decode(&mut load, |_, text| CatchUp::parse(text))?,
            frame_rate: FRAME_RATE.decode(&mut load, |_, text| FrameRate::parse(text))?,
            slide: SLIDE.decode(&mut load, read)?,
            tier_probe: TIER_PROBE.decode(&mut load, read)?,
            notices: NOTICES.decode(&mut load, read)?,
            revealed: REVEALED.decode(&mut load, read)?,
            shown: SHOWN.decode(&mut load, read)?,
            lighting: LIGHTING.decode(&mut load, |_, text| Lighting::parse(text))?,
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

/// Parses a tuning file and attaches its name and the failing key to a parse error.
pub fn read<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, String> {
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
    for pair in fly.draw.local.ease.windows(2) {
        if pair[1].time < pair[0].time {
            return Err((
                "fly.draw.local.ease",
                "keyframes must be sorted by ascending time",
            ));
        }
    }
    for pair in fly.draw.opponent.ease.windows(2) {
        if pair[1].time < pair[0].time {
            return Err((
                "fly.draw.opponent.ease",
                "keyframes must be sorted by ascending time",
            ));
        }
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

/// Deserializes a finite number.
pub fn finite<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |_| true, "a finite number")
}

/// Deserializes a finite number greater than or equal to zero.
pub fn nonnegative<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v >= 0.0, "a finite nonnegative number")
}

/// Deserializes a finite number greater than zero.
pub fn positive<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| v > 0.0, "a finite positive number")
}

pub(super) fn fraction<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| (0.0..=1.0).contains(&v), "a number from 0 to 1")
}

pub(super) fn before_end<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
    number(de, |v| (0.0..1.0).contains(&v), "a number from 0 up to 1")
}

/// Deserializes a number greater than zero and at most one.
pub fn positive_fraction<'de, D: Deserializer<'de>>(de: D) -> Result<f32, D::Error> {
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

pub(super) fn quaternion<'de, D: Deserializer<'de>>(de: D) -> Result<Quat, D::Error> {
    let raw = <[f32; 4]>::deserialize(de)?;
    if !raw.iter().all(|v| v.is_finite()) {
        return Err(D::Error::custom("expected four finite components"));
    }
    let quat = Quat::from_array(raw);
    if quat.length_squared() <= 1e-6 {
        return Err(D::Error::custom("expected a non-zero quaternion"));
    }
    Ok(quat.normalize())
}
