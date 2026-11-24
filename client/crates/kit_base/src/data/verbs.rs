//! The verb tuning in `verbs.json`. Distances, angles and shares follow each verb's motion.

use glam::{Quat, Vec2};
use serde::Deserialize;

use super::tuning::{
    before_end, finite, fraction, interior, nonnegative, nonzero_count, positive,
    positive_fraction, quaternion, vector,
};
use crate::pose::{HermiteCurve, HermiteKey, MAX_HERMITE_KEYS};

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
    pub draw: DrawSplines,
    pub paths: FlightSplines,
}

/// Authored multi-node splines for flights between resting zones.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FlightSplines {
    pub place_near: StraightSpline,
    pub place_far: ApexSpline4,
    pub send_near: RelativeSpline1,
    pub send_far: RelativeSpline2,
    pub settle_default: ApexSpline3,
    pub settle_gliding: ApexSpline3,
    pub fetch_near: StraightSpline,
    pub fetch_far: ApexSpline3,
    pub return_drift: StraightSpline,
}

/// A two-node straight spline driven by a three-keyframe Hermite curve and endpoint depth weight.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StraightSpline {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub ease: [HermiteKeyData; 3],
}

impl StraightSpline {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        curve_of_3(self.ease)
    }
}

/// One interior camera-space waypoint for a three-node spline.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApexNode {
    #[serde(deserialize_with = "vector")]
    pub ndc: Vec2,
    #[serde(deserialize_with = "fraction")]
    pub depth_share: f32,
    #[serde(deserialize_with = "positive")]
    pub max_depth: f32,
    #[serde(deserialize_with = "positive")]
    pub rel_scale: f32,
    #[serde(deserialize_with = "quaternion")]
    pub rotation: Quat,
}

/// A three-node camera-space spline with a three-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApexSpline3 {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub node: ApexNode,
    pub ease: [HermiteKeyData; 3],
}

impl ApexSpline3 {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        curve_of_3(self.ease)
    }
}

/// A three-node camera-space spline with a four-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApexSpline4 {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub node: ApexNode,
    pub ease: [HermiteKeyData; 4],
}

impl ApexSpline4 {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        HermiteCurve {
            keys: self.ease.map(HermiteKey::from),
            len: 4,
        }
    }
}

/// One interior waypoint placed proportionally between start and end NDC positions.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelativeNode {
    #[serde(deserialize_with = "vector")]
    pub ndc_share: Vec2,
    #[serde(deserialize_with = "fraction")]
    pub depth_share: f32,
    #[serde(deserialize_with = "positive")]
    pub rel_scale: f32,
    #[serde(deserialize_with = "quaternion")]
    pub rotation: Quat,
}

/// A three-node endpoint-relative spline with a three-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelativeSpline1 {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub node: RelativeNode,
    pub ease: [HermiteKeyData; 3],
}

impl RelativeSpline1 {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        curve_of_3(self.ease)
    }
}

/// A four-node endpoint-relative spline with a three-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelativeSpline2 {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub nodes: [RelativeNode; 2],
    pub ease: [HermiteKeyData; 3],
}

impl RelativeSpline2 {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        curve_of_3(self.ease)
    }
}

fn curve_of_3(ease: [HermiteKeyData; 3]) -> HermiteCurve {
    let mut keys = [HermiteKey::default(); MAX_HERMITE_KEYS];
    for (slot, key) in keys.iter_mut().zip(ease) {
        *slot = key.into();
    }
    HermiteCurve { keys, len: 3 }
}

/// Authored multi-node splines for card flights.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DrawSplines {
    pub local: LocalDrawSpline,
    pub opponent: OpponentDrawSpline,
}

/// One keyframe of a piecewise cubic Hermite timing curve in `verbs.json`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HermiteKeyData {
    #[serde(deserialize_with = "fraction")]
    pub time: f32,
    #[serde(deserialize_with = "fraction")]
    pub value: f32,
    #[serde(deserialize_with = "finite")]
    pub in_slope: f32,
    #[serde(deserialize_with = "finite")]
    pub out_slope: f32,
}

impl From<HermiteKeyData> for HermiteKey {
    fn from(data: HermiteKeyData) -> Self {
        Self {
            time: data.time,
            value: data.value,
            in_slope: data.in_slope,
            out_slope: data.out_slope,
        }
    }
}

/// One interior control node of a six-node card spline, specified in camera space.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocalDrawNode {
    #[serde(deserialize_with = "vector")]
    pub ndc: Vec2,
    #[serde(deserialize_with = "fraction")]
    pub depth_share: f32,
    #[serde(deserialize_with = "positive")]
    pub max_depth: f32,
    #[serde(deserialize_with = "positive")]
    pub rel_scale: f32,
    #[serde(deserialize_with = "quaternion")]
    pub rotation: Quat,
}

/// A six-node spline (endpoints plus four interior nodes) and four-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LocalDrawSpline {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub nodes: [LocalDrawNode; 4],
    pub ease: [HermiteKeyData; 4],
}

impl LocalDrawSpline {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        HermiteCurve {
            keys: self.ease.map(HermiteKey::from),
            len: 4,
        }
    }
}

/// One interior control node of a four-node card spline, specified in camera space.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpponentDrawNode {
    #[serde(deserialize_with = "vector")]
    pub ndc: Vec2,
    #[serde(deserialize_with = "fraction")]
    pub depth_share: f32,
    #[serde(deserialize_with = "positive")]
    pub rel_scale: f32,
    #[serde(deserialize_with = "fraction")]
    pub pitch_blend: f32,
}

/// A four-node spline (endpoints plus two interior nodes) and three-keyframe Hermite curve.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpponentDrawSpline {
    #[serde(deserialize_with = "positive")]
    pub from_rel_scale: f32,
    pub nodes: [OpponentDrawNode; 2],
    pub ease: [HermiteKeyData; 3],
}

impl OpponentDrawSpline {
    /// Converts the authored keyframes into a `HermiteCurve`.
    #[must_use]
    pub fn curve(&self) -> HermiteCurve {
        let mut keys = [HermiteKey::default(); MAX_HERMITE_KEYS];
        for (slot, key) in keys.iter_mut().zip(self.ease) {
            *slot = key.into();
        }
        HermiteCurve { keys, len: 3 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bows {
    pub cast: Bowing,
    pub theirs: Bowing,
    pub played: Bowing,
    /// The web uses this for emerging cards. the client currently uses a straight path.
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
