//! The verb tuning in `verbs.json`. Distances, angles and shares follow each verb's motion.

use glam::Vec2;
use serde::Deserialize;

use super::tuning::{
    before_end, finite, fraction, interior, nonnegative, nonzero_count, positive,
    positive_fraction, vector,
};

/// Tuning for each of the verbs in `verbs.json`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Verbs {
    pub fly: Fly,
    pub strike: Strike,
    pub count: Count,
    pub emerge: Emerge,
    pub flip: Flip,
    pub showcase: Showcase,
    pub lift: Lift,
    pub streak: Streak,
    pub tap: Tap,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fly {
    #[serde(deserialize_with = "nonnegative")]
    pub arc: f32,
    pub bows: Bows,
    pub thrown: Thrown,
    pub turn: Turns,
    pub held: Held,
    #[serde(deserialize_with = "before_end")]
    pub held_sink: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bows {
    pub cast: Bowing,
    pub theirs: Bowing,
    pub played: Bowing,
    /// The web uses this for emerging cards. Beryl 3D currently uses a straight path.
    pub lifts: Bowing,
    pub drawn: Bowing,
    pub taken: Bowing,
}

/// A flight's offset from its straight path, the peak of that offset, and its lean in degrees.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bowing {
    #[serde(deserialize_with = "nonnegative")]
    pub share: f32,
    #[serde(deserialize_with = "interior")]
    pub peak: f32,
    #[serde(deserialize_with = "finite")]
    pub lean: f32,
    #[serde(deserialize_with = "fraction")]
    pub back: f32,
}

/// The height, scale and timing of an object settling onto the table.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Thrown {
    #[serde(deserialize_with = "nonnegative")]
    pub height: f32,
    #[serde(deserialize_with = "interior")]
    pub risen: f32,
    #[serde(deserialize_with = "interior")]
    pub held: f32,
    #[serde(deserialize_with = "positive")]
    pub scale: f32,
    #[serde(deserialize_with = "interior")]
    pub shrunk: f32,
    #[serde(deserialize_with = "positive")]
    pub launch: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Turns {
    pub cast: Turn,
    pub draw: Turn,
}

/// When a card turns edge-on and the share of the flight spent turning on each side of it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    #[serde(deserialize_with = "interior")]
    pub at: f32,
    #[serde(deserialize_with = "interior")]
    pub each: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Held {
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "interior")]
    pub r#in: f32,
    #[serde(deserialize_with = "interior")]
    pub hold: f32,
    #[serde(deserialize_with = "finite")]
    pub lean: f32,
    pub turn: Turn,
    pub bow: Bowing,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Strike {
    #[serde(deserialize_with = "fraction")]
    pub wind: f32,
    #[serde(deserialize_with = "fraction")]
    pub stay: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub rear: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub lift: f32,
    #[serde(deserialize_with = "finite")]
    pub lean: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub near: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub least: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Count {
    #[serde(deserialize_with = "nonnegative")]
    pub drift: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Emerge {
    #[serde(deserialize_with = "positive")]
    pub start_scale: f32,
    #[serde(deserialize_with = "vector")]
    pub start_offset: Vec2,
}

/// How a card rises, grows, turns and settles when it flips in place.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Flip {
    #[serde(deserialize_with = "nonnegative")]
    pub rise: f32,
    #[serde(deserialize_with = "positive")]
    pub scale: f32,
    #[serde(deserialize_with = "interior")]
    pub grown: f32,
    #[serde(deserialize_with = "interior")]
    pub settle: f32,
}

/// How a revealed card grows and fades into its place.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Showcase {
    #[serde(deserialize_with = "positive")]
    pub size: f32,
    #[serde(deserialize_with = "positive_fraction")]
    pub seen: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Lift {
    pub pile: LiftPile,
    pub hand: LiftHand,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiftPile {
    #[serde(deserialize_with = "vector")]
    pub beside: Vec2,
    #[serde(deserialize_with = "nonnegative")]
    pub step: f32,
    #[serde(deserialize_with = "nonzero_count")]
    pub columns: usize,
    #[serde(deserialize_with = "nonnegative")]
    pub rise: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiftHand {
    #[serde(deserialize_with = "vector")]
    pub at: Vec2,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub step: f32,
    #[serde(deserialize_with = "nonzero_count")]
    pub columns: usize,
    #[serde(deserialize_with = "finite")]
    pub lean_degrees: f32,
    #[serde(deserialize_with = "positive")]
    pub depth: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub drop: f32,
    #[serde(deserialize_with = "vector")]
    pub theirs_at: Vec2,
    pub opening: LiftOpening,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiftOpening {
    #[serde(deserialize_with = "vector")]
    pub at: Vec2,
    #[serde(deserialize_with = "positive")]
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Streak {
    #[serde(deserialize_with = "nonnegative")]
    pub bow: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub rise: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub fan: f32,
    #[serde(deserialize_with = "fraction")]
    pub tail: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub head: f32,
    #[serde(deserialize_with = "positive")]
    pub least: f32,
    #[serde(deserialize_with = "positive")]
    pub most: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub more: f32,
    #[serde(deserialize_with = "positive")]
    pub units: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub ribbon: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub halo: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub orb: f32,
    #[serde(deserialize_with = "nonnegative")]
    pub dot: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tap {
    #[serde(deserialize_with = "nonnegative")]
    pub near: f32,
}
