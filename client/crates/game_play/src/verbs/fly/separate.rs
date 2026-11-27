//! Mid-air separation for staggered flights.
//!
//! When one beat flies several cards in quick succession into a narrow or piled row, later cards
//! leave along the same short corridor while earlier cards are still elevated in flight, so the
//! cards overlap in `MovingOpaque` and cover each other for most of their flight. When an earlier
//! crossing flight overlaps this card across too many frames, we bow this card's path laterally
//! (preferring the direction toward the center of the window and clamped inside the safe area) so
//! both cards remain visible in mid-air. When a later step retargets an in-flight card onto a
//! newly piled destination, `reseparate_retargeted` re-checks that flight against earlier crossing
//! flights so retargeted flights do not collapse back onto the same straight corridor.

use beryl_protocol::script::Subject;
use glam::{Vec2, Vec3};

use super::{clamped_to_safe_area, kept_in_window, straight_spline as build_straight_spline};
use crate::play::record::SegmentRecord;
use crate::verbs::StepIn;
use game_table::table::{Camera, Table};
use kit_base::data::verbs::StraightSpline;
use kit_base::pose::{Bow, Follows, Interval, Pose, Segment, Shape, Spline, SplineNode};
use kit_space::motion::Track;

/// Interior time samples taken across a flight to measure mid-air overlap with earlier flights.
const SAMPLES: usize = 15;

/// Overlap ratio above which a sample counts as covered by peer flights (strictly tighter than
/// `1 - audit.seen` = 0.50 so separated flights keep margin).
const COVER_LIMIT: f32 = 0.45;

/// Default flights covered in at most this many samples (3 of 15 = 20%) stay untouched.
const ALLOWED_COVERED: usize = SAMPLES / 4;

/// Screen-space axis-aligned bounding box of a projected card.
#[derive(Clone, Copy)]
struct Box2 {
    min: Vec2,
    max: Vec2,
}

impl Box2 {
    fn of(camera: &Camera, pose: &Pose) -> Self {
        let (corners, _) = camera.card_box(pose);
        let min = corners.into_iter().reduce(Vec2::min).unwrap_or_default();
        let max = corners.into_iter().reduce(Vec2::max).unwrap_or_default();
        Self { min, max }
    }

    fn area(self) -> f32 {
        (self.max.x - self.min.x).max(0.0) * (self.max.y - self.min.y).max(0.0)
    }

    fn intersection_area(self, other: Self) -> f32 {
        let w = (self.max.x.min(other.max.x) - self.min.x.max(other.min.x)).max(0.0);
        let h = (self.max.y.min(other.max.y) - self.min.y.max(other.min.y)).max(0.0);
        w * h
    }

    fn span(self) -> f32 {
        (self.max.x - self.min.x).max(self.max.y - self.min.y)
    }
}

fn is_crossing_segment(segment: &Segment) -> bool {
    segment
        .step
        .is_some_and(|step| step.follows == Follows::Start)
        && segment.shape != Shape::Fading
        && (segment.from.position != segment.to.position || segment.arc != 0.0)
}

/// Earlier crossing flights whose segments overlap this flight, pre-sampled at `SAMPLES` interior
/// timestamps.
struct PeerSamples {
    times: [u64; SAMPLES],
    boxes: [Vec<Box2>; SAMPLES],
}

impl PeerSamples {
    fn collect_from_places(step: &StepIn<'_>, subject: Subject) -> Option<Self> {
        let duration = step.own.duration();
        if duration == 0 {
            return None;
        }
        let mut peers: Vec<Subject> = Vec::new();
        step.places.crossing_peers(subject, step.own, &mut peers);
        if peers.is_empty() {
            return None;
        }

        let camera = step.places.camera();
        let mut times = [0u64; SAMPLES];
        let mut boxes = std::array::from_fn(|_| Vec::new());
        for (i, (time, slot)) in times.iter_mut().zip(boxes.iter_mut()).enumerate() {
            let t = step.own.from + (duration * (i as u64 + 1)) / (SAMPLES as u64 + 1);
            *time = t;
            for &peer in &peers {
                if !step.places.crossing(peer, t) {
                    continue;
                }
                let Some(pose) = step.places.pose(peer, t) else {
                    continue;
                };
                if pose.opacity > 0.01 {
                    slot.push(Box2::of(camera, &pose));
                }
            }
        }
        if boxes.iter().all(Vec::is_empty) {
            return None;
        }
        Some(Self { times, boxes })
    }

    fn collect_from_tracks(
        tracks: &[Track],
        slot: usize,
        own: Interval,
        camera: &Camera,
    ) -> Option<Self> {
        let duration = own.duration();
        if duration == 0 {
            return None;
        }
        let mut peers: Vec<usize> = Vec::new();
        for (other, track) in tracks.iter().enumerate() {
            if other == slot || track.segments().is_empty() {
                continue;
            }
            let earlier_overlapping = track.segments().iter().any(|seg| {
                is_crossing_segment(seg)
                    && seg.start < own.to
                    && seg.end() > own.from
                    && (seg.start < own.from || (seg.start == own.from && other < slot))
            });
            if earlier_overlapping {
                peers.push(other);
            }
        }
        if peers.is_empty() {
            return None;
        }

        let mut times = [0u64; SAMPLES];
        let mut boxes = std::array::from_fn(|_| Vec::new());
        for (i, (time, bucket)) in times.iter_mut().zip(boxes.iter_mut()).enumerate() {
            let t = own.from + (duration * (i as u64 + 1)) / (SAMPLES as u64 + 1);
            *time = t;
            for &peer in &peers {
                if !tracks[peer].crossing(t) {
                    continue;
                }
                let pose = tracks[peer].pose_at(t);
                if pose.opacity > 0.01 {
                    bucket.push(Box2::of(camera, &pose));
                }
            }
        }
        if boxes.iter().all(Vec::is_empty) {
            return None;
        }
        Some(Self { times, boxes })
    }

    /// Scores `segment` by `(covered_samples, total_cover)` against the sampled peer flights.
    fn score(&self, segment: &Segment, camera: &Camera) -> (usize, f32) {
        let mut covered = 0usize;
        let mut total = 0.0_f32;
        for (&t, peers) in self.times.iter().zip(self.boxes.iter()) {
            if peers.is_empty() {
                continue;
            }
            let pose = segment.pose_at(t);
            let mine = Box2::of(camera, &pose);
            let my_area = mine.area();
            if my_area <= 1.0 {
                continue;
            }
            let mut sum_mine = 0.0_f32;
            let mut max_peer = 0.0_f32;
            for &peer in peers {
                let inter = mine.intersection_area(peer);
                if inter <= 0.0 {
                    continue;
                }
                sum_mine += inter / my_area;
                let peer_area = peer.area();
                if peer_area > 1.0 {
                    max_peer = max_peer.max(inter / peer_area);
                }
            }
            let cover = sum_mine.min(1.0).max(max_peer);
            if cover > COVER_LIMIT {
                covered += 1;
            }
            total += cover;
        }
        (covered, total)
    }
}

/// Candidate screen-space mid-flight offsets (in points) to separate overlapping staggered flights,
/// ordered to prefer bowing toward the open center of the window with the smallest clear arc.
fn candidate_offsets(from: &Pose, to: &Pose, camera: &Camera) -> [Vec2; 6] {
    let from_s = camera.to_screen(from.position);
    let to_s = camera.to_screen(to.position);
    let mid_s = from_s.midpoint(to_s);
    let center = Vec2::new(
        camera.window.size.width * 0.5,
        camera.window.size.height * 0.5,
    );
    let toward_center = (center - mid_s).normalize_or(Vec2::new(0.0, -1.0));
    let way = to_s - from_s;
    let mut perp = if way.length() > 1.0 {
        let dir = way.normalize();
        Vec2::new(-dir.y, dir.x)
    } else {
        toward_center
    };
    if perp.dot(center - mid_s) < 0.0 {
        perp = -perp;
    }
    let mid_pose = Pose {
        position: from.position.midpoint(to.position),
        rotation: from.rotation.slerp(to.rotation, 0.5),
        scale: f32::midpoint(from.scale, to.scale),
        ..*from
    };
    let span = Box2::of(camera, &mid_pose).span().max(24.0);
    let near = span * 1.15;
    let far = span * 2.1;
    [
        perp * near,
        toward_center * near,
        -perp * near,
        perp * far,
        toward_center * far,
        -perp * far,
    ]
}

fn better(a: (usize, f32), b: (usize, f32)) -> bool {
    a.0 < b.0 || (a.0 == b.0 && a.1 + 0.05 < b.1)
}

fn separate_straight_spline_with(
    segment: Segment,
    spline: Spline,
    peers: &PeerSamples,
    camera: &Camera,
) -> Option<Shape> {
    let base_score = peers.score(&segment, camera);
    if base_score.0 <= ALLOWED_COVERED {
        return None;
    }

    let (from, to) = (segment.from, segment.to);
    let from_depth = camera.depth(from.position);
    let to_depth = camera.depth(to.position);
    let depth = f32::midpoint(from_depth, to_depth);
    let from_seen = from.scale / from_depth;
    let to_seen = to.scale / to_depth;
    let seen_scale = f32::midpoint(from_seen, to_seen);
    let rotation = from.rotation.slerp(to.rotation, 0.5);
    let camera_back = camera.axes()[2];
    let from_rel_scale = (1.0 / spline.from_weight()).max(1e-4);
    let weight = 1.0 / f32::midpoint(from_rel_scale, 1.0);

    let mut best_shape = None;
    let mut best_score = base_score;
    for delta in candidate_offsets(&from, &to, camera) {
        let mut mid = Pose {
            position: from.position.midpoint(to.position),
            rotation,
            scale: seen_scale * depth,
            turn: from.turn,
            ..Pose::default()
        };
        camera.move_in_screen(&mut mid, delta);
        let clamped = clamped_to_safe_area(mid, camera);
        let node = SplineNode {
            position: clamped.position,
            rotation,
            seen_scale,
            weight,
        };
        let shape = Shape::Spline(spline.with_interior(node, &to, camera.eye, camera_back));
        let candidate = Segment { shape, ..segment };
        let score = peers.score(&candidate, camera);
        if better(score, best_score) {
            best_score = score;
            best_shape = Some(shape);
            if best_score.0 == 0 {
                break;
            }
        }
    }
    best_shape
}

fn separate_bow_with(
    segment: Segment,
    peers: &PeerSamples,
    camera: &Camera,
    target: f32,
) -> Option<Segment> {
    let base_score = peers.score(&segment, camera);
    if base_score.0 <= ALLOWED_COVERED {
        return None;
    }

    let [right, up, back] = camera.axes();
    let base_bow = match segment.shape {
        Shape::Line => Bow {
            offset: Vec3::ZERO,
            sharpness: 1.0,
            lean: 0.0,
            axis: back,
        },
        Shape::Bowed(bow) => bow,
        _ => return None,
    };
    let middle = segment.from.position.midpoint(segment.to.position);
    let points_per_mm = (camera.to_screen(middle + right).x - camera.to_screen(middle).x).max(1e-4);

    let mut best_segment = None;
    let mut best_score = base_score;
    for delta in candidate_offsets(&segment.from, &segment.to, camera) {
        let world_delta = right * (delta.x / points_per_mm) - up * (delta.y / points_per_mm);
        let bow = Bow {
            offset: base_bow.offset + world_delta,
            ..base_bow
        };
        let candidate = kept_in_window(
            Segment {
                shape: Shape::Bowed(bow),
                ..segment
            },
            camera,
            target,
        );
        let score = peers.score(&candidate, camera);
        if better(score, best_score) {
            best_score = score;
            best_segment = Some(candidate);
            if best_score.0 == 0 {
                break;
            }
        }
    }
    best_segment
}

/// Separates a two-node `straight_spline` (used for exile transfers, local land plays, and local
/// library-to-battlefield flights) when an earlier flight covers it in mid-air.
pub(super) fn straight_spline(
    step: &StepIn<'_>,
    subject: Subject,
    between: Segment,
    spec: &StraightSpline,
) -> Shape {
    let default_shape = build_straight_spline(step, between.to, spec);
    if step.places.layout().bands.is_none() {
        return default_shape;
    }
    let Shape::Spline(spline) = default_shape else {
        return default_shape;
    };
    let Some(peers) = PeerSamples::collect_from_places(step, subject) else {
        return default_shape;
    };
    let default_segment = Segment {
        shape: default_shape,
        ..between
    };
    separate_straight_spline_with(default_segment, spline, &peers, step.places.camera())
        .unwrap_or(default_shape)
}

/// Separates a straight or bowed flight when an earlier flight covers it in mid-air.
pub(super) fn bowed(step: &StepIn<'_>, subject: Subject, segment: Segment) -> Segment {
    if step.places.layout().bands.is_none() {
        return segment;
    }
    let Some(peers) = PeerSamples::collect_from_places(step, subject) else {
        return segment;
    };
    separate_bow_with(
        segment,
        &peers,
        step.places.camera(),
        step.places.layout().target,
    )
    .unwrap_or(segment)
}

/// Separates a staggered `local_draw_spline` on compact layouts when earlier flights cover it
/// around the hold spot, by trying screen-space offsets for the hold waypoint.
pub(super) fn local_draw(
    step: &StepIn<'_>,
    subject: Subject,
    between: Segment,
    build: impl Fn(Vec2) -> Shape,
) -> Shape {
    let default_shape = build(Vec2::ZERO);
    if step.places.layout().bands.is_none() {
        return default_shape;
    }
    let Some(peers) = PeerSamples::collect_from_places(step, subject) else {
        return default_shape;
    };
    let camera = step.places.camera();
    let default_segment = Segment {
        shape: default_shape,
        ..between
    };
    let mut best_score = peers.score(&default_segment, camera);
    if best_score.0 <= ALLOWED_COVERED {
        return default_shape;
    }

    let span = camera.window.size.width.min(camera.window.size.height) * 0.42;
    let offsets = [
        Vec2::new(-span * 0.65, 0.0),
        Vec2::new(0.0, -span * 0.65),
        Vec2::new(-span * 0.65, -span * 0.65),
        Vec2::new(-span * 1.15, 0.0),
        Vec2::new(-span * 1.15, -span * 0.65),
        Vec2::new(0.0, -span * 1.15),
        Vec2::new(-span * 0.65, -span * 1.15),
        Vec2::new(span * 0.65, 0.0),
        Vec2::new(0.0, span * 0.65),
    ];
    let mut best_shape = default_shape;
    for offset in offsets {
        let shape = build(offset);
        let candidate = Segment { shape, ..between };
        let score = peers.score(&candidate, camera);
        if better(score, best_score) {
            best_score = score;
            best_shape = shape;
            if best_score.0 == 0 {
                break;
            }
        }
    }
    best_shape
}

/// Re-checks any in-flight crossing segment of `slot` that `retarget` just redirected at `from`
/// against earlier crossing tracks, updating both the track and `records` if the new destination
/// puts it directly behind an earlier flight.
pub(crate) fn reseparate_retargeted(
    (tracks, records): (&mut [Track], &mut [SegmentRecord]),
    (slot, from): (usize, u64),
    table: &Table,
) {
    if table.layout().bands.is_none() {
        return;
    }
    let Some(track) = tracks.get(slot) else {
        return;
    };
    let camera = table.camera();
    let target = table.layout().target;
    let segments = track.segments();
    let mut updates: Vec<(usize, Segment)> = Vec::new();
    for (index, &segment) in segments.iter().enumerate() {
        if segment.end() <= from || !is_crossing_segment(&segment) {
            continue;
        }
        // Do not alter a flight's interior curve once a later segment has already captured a
        // mid-flight starting pose from it, or that later segment would jump.
        if segments[index + 1..]
            .iter()
            .any(|later| later.start < segment.end())
        {
            continue;
        }
        let own = Interval {
            from: segment.start,
            to: segment.end(),
        };
        let Some(peers) = PeerSamples::collect_from_tracks(tracks, slot, own, camera) else {
            continue;
        };
        let replacement = match segment.shape {
            Shape::Spline(spline) if spline.is_straight() => {
                separate_straight_spline_with(segment, spline, &peers, camera)
                    .map(|shape| Segment { shape, ..segment })
            }
            Shape::Line | Shape::Bowed(_) => separate_bow_with(segment, &peers, camera, target),
            _ => None,
        };
        if let Some(new_segment) = replacement {
            updates.push((index, new_segment));
        }
    }
    if updates.is_empty() {
        return;
    }
    for (index, new_segment) in updates {
        let start = tracks[slot].segments()[index].start;
        tracks[slot].segments_mut()[index] = new_segment;
        for record in records.iter_mut() {
            if record.slot == slot
                && record.segment.start == start
                && record.segment.step == new_segment.step
            {
                record.segment = new_segment;
            }
        }
    }
}
