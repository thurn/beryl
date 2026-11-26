//! Poses from segments and tracks of segments.

use glam::{Quat, Vec3};
use kit_base::pose::{Curve, Follows, Interval, Pose, Segment, Shape, StepTime};
use kit_space::motion::{Full, SEGMENTS, Track, pose_at};

const MS: u64 = 1_000_000;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn at(x: f32) -> Pose {
    Pose {
        position: Vec3::new(x, 0.0, 0.0),
        ..Pose::default()
    }
}

fn segment(start: u64, duration: u64, from: Pose, to: Pose) -> Segment {
    Segment::between(
        from,
        to,
        Interval {
            from: start,
            to: start + duration,
        },
        Curve::EASE_OUT,
    )
}

#[test]
fn a_segment_starts_at_its_start_pose_and_rests_at_its_end_pose() {
    let to = Pose {
        position: Vec3::new(100.0, 50.0, 0.0),
        rotation: Quat::from_rotation_z(1.0),
        scale: 2.0,
        opacity: 0.5,
        turn: 1.0,
        skew: 0.0,
        shear: 0.25,
    };
    let segments = [segment(100 * MS, 200 * MS, at(0.0), to)];
    assert_eq!(pose_at(&segments, 0), at(0.0), "before it begins");
    assert_eq!(pose_at(&segments, 100 * MS), at(0.0), "as it begins");
    assert_eq!(pose_at(&segments, 300 * MS), to, "as it ends");
    assert_eq!(pose_at(&segments, 9_000 * MS), to, "long after");
}

#[test]
fn halfway_through_its_time_a_segment_is_where_its_curve_says() {
    let to = Pose {
        opacity: 0.0,
        turn: 1.0,
        ..at(100.0)
    };
    let mut segments = [segment(0, 200 * MS, at(0.0), to)];
    // CSS's ease-out, cubic-bezier(0, 0, 0.58, 1), at half its time, as `motion`'s `easeOut(0.5)`
    // gives it; ease-in mirrors it.
    let middle = pose_at(&segments, 100 * MS);
    assert!(close(middle.position.x, 68.464_32), "{middle:?}");
    assert!(close(middle.opacity, 1.0 - 0.684_643_2), "{middle:?}");
    assert!(close(middle.turn, 0.684_643_2), "{middle:?}");
    segments[0].curve = Curve::EASE_IN;
    assert!(close(
        pose_at(&segments, 100 * MS).position.x,
        100.0 - 68.464_32
    ));
    segments[0].curve = Curve::LINEAR;
    assert!(close(pose_at(&segments, 50 * MS).position.x, 25.0));
    segments[0].curve = Curve::EASE_IN_OUT;
    assert!(close(pose_at(&segments, 100 * MS).position.x, 50.0));
}

#[test]
fn an_arc_rises_highest_at_the_middle_of_its_time() {
    let mut flight = segment(0, 400 * MS, at(0.0), at(300.0));
    flight.arc = 40.0;
    let height = |t: u64| pose_at(&[flight], t * MS).position.z;
    assert!(close(height(200), 40.0));
    assert!(height(100) < height(200) && height(300) < height(200));
    assert!(close(height(100), height(300)));
    assert!(close(height(0), 0.0) && close(height(400), 0.0));
}

#[test]
fn the_latest_segment_to_have_begun_gives_the_pose() {
    let segments = [
        segment(0, 1_000 * MS, at(0.0), at(100.0)),
        segment(200 * MS, 0, at(500.0), at(500.0)),
    ];
    assert!(pose_at(&segments, 100 * MS).position.x < 100.0);
    assert_eq!(pose_at(&segments, 200 * MS), at(500.0));
    assert_eq!(pose_at(&segments, 900 * MS), at(500.0));
    assert_eq!(pose_at(&[], 5), Pose::default());
}

#[test]
fn a_track_keeps_its_segments_in_start_order() {
    let mut track = Track::default();
    for start in [300, 100, 200, 100] {
        track
            .push(segment(start * MS, 0, at(0.0), at(start as f32)), 0)
            .unwrap();
    }
    let starts: Vec<u64> = track.segments().iter().map(|s| s.start / MS).collect();
    assert_eq!(starts, [100, 100, 200, 300]);
}

/// A hold between two moves of the same step pauses the object. A hold between different
/// steps, or after the last move, does not.
#[test]
fn a_card_held_within_its_steps_act_is_paused() {
    let of = |number, mut segment: Segment| {
        segment.step = Some(StepTime {
            number,
            follows: Follows::Start,
        });
        segment
    };
    let mut track = Track::default();
    for segment in [
        of(1, segment(0, 100 * MS, at(0.0), at(1.0))),
        of(1, segment(200 * MS, 100 * MS, at(1.0), at(2.0))),
        of(2, segment(500 * MS, 100 * MS, at(2.0), at(3.0))),
    ] {
        track.push(segment, 0).unwrap();
    }
    assert!(track.paused(150 * MS), "held between two moves of step 1");
    assert!(!track.moving(150 * MS), "nothing moves it at 150 ms");
    assert!(!track.paused(400 * MS), "between step 1 and step 2");
    assert!(!track.paused(700 * MS), "after the last move");
}

/// A card that vanishes (a countered spell crumbling under the spell that countered it) stays
/// where it was until its step ends, so it keeps its depth among the cards round it and is
/// not drawn over them as a card carried across the table is. Its segment still ends at the
/// pile it goes to.
#[test]
fn a_card_fading_where_it_lies_is_not_crossing_the_table() {
    let step = Some(StepTime {
        number: 1,
        follows: Follows::Start,
    });
    let mut fading = Track::default();
    let vanish = Segment {
        shape: Shape::Fading,
        step,
        ..segment(0, 300 * MS, at(0.0), at(500.0))
    };
    fading.push(vanish, 0).unwrap();
    assert!(!fading.crossing(150 * MS), "it fades on the stack");
    assert_eq!(fading.pose_at(150 * MS).position, at(0.0).position);
    let mut flying = Track::default();
    flying
        .push(
            Segment {
                step,
                ..segment(0, 300 * MS, at(0.0), at(500.0))
            },
            0,
        )
        .unwrap();
    assert!(
        flying.crossing(150 * MS),
        "a flight to the pile crosses the table"
    );
}

/// A full track drops only segments that have ended, keeping the latest of them, which still
/// gives the pose until the next begins. Segments in the future stay.
#[test]
fn a_full_track_drops_ended_segments_but_the_latest() {
    let mut track = Track::default();
    for i in 0..SEGMENTS as u64 {
        track
            .push(segment(i * 100 * MS, 50 * MS, at(0.0), at(i as f32)), 0)
            .unwrap();
    }
    // At 260 ms the segments starting at 0, 100 and 200 ms have ended.
    track
        .push(segment(5_000 * MS, 0, at(0.0), at(99.0)), 260 * MS)
        .unwrap();
    let starts: Vec<u64> = track.segments().iter().map(|s| s.start / MS).collect();
    let kept: Vec<u64> = (2..SEGMENTS as u64)
        .map(|i| i * 100)
        .chain([5_000])
        .collect();
    assert_eq!(starts, kept);
    assert_eq!(track.pose_at(270 * MS), at(2.0));
    track
        .push(segment(6_000 * MS, 0, at(0.0), at(0.0)), 260 * MS)
        .unwrap();
    assert_eq!(track.segments().len(), SEGMENTS);
    // Only one segment has ended, and nothing that lies ahead may go.
    let refused = track.push(segment(7_000 * MS, 0, at(0.0), at(0.0)), 260 * MS);
    assert_eq!(refused, Err(Full));
    assert_eq!(track.segments().len(), SEGMENTS);
}

/// A card in a pile takes a segment that only changes its height each time a card joins the
/// pile. A full track drops those before the segments that carry the card, so the card still
/// stands where it started until its flight begins.
#[test]
fn a_full_track_drops_ended_layer_changes_before_flights() {
    let high = |z: f32| Pose {
        position: Vec3::new(0.0, 0.0, z),
        ..Pose::default()
    };
    let mut track = Track::default();
    track.push(segment(0, 0, at(0.0), at(0.0)), 0).unwrap();
    track
        .push(segment(100 * MS, 50 * MS, at(0.0), at(5.0)), 0)
        .unwrap();
    for i in 2..SEGMENTS as u64 {
        track
            .push(
                segment(i * 100 * MS, 0, high(i as f32 - 2.0), high(i as f32 - 1.0)),
                0,
            )
            .unwrap();
    }
    assert_eq!(track.segments().len(), SEGMENTS);
    track
        .push(segment(5_000 * MS, 0, high(14.0), high(15.0)), 5_000 * MS)
        .unwrap();
    let starts: Vec<u64> = track.segments().iter().map(|s| s.start / MS).collect();
    assert_eq!(starts, vec![0, 100, 5_000]);
    assert_eq!(
        track.pose_at(50 * MS),
        at(0.0),
        "it stands where it started"
    );
    assert_eq!(
        track.pose_at(200 * MS),
        at(5.0),
        "and the flight still moves it"
    );
}

/// What is drawn on a card (its plaque, its marks) lies on the side of the card that faces up:
/// flat on its face while face up and on its back while face down, and edge-on with the card
/// half way through turning over.
#[test]
fn what_is_drawn_on_a_card_turns_over_with_it() {
    let normal = |turn: f32| {
        let pose = Pose {
            turn,
            ..Pose::default()
        };
        pose.upward().transform_vector3(glam::Vec3::Z)
    };
    for turn in [0.0, 1.0] {
        assert!(
            (normal(turn) - glam::Vec3::Z).length() < 1e-5,
            "flat and facing up at turn {turn}: {}",
            normal(turn)
        );
    }
    assert!(
        normal(0.5).z.abs() < 1e-5,
        "edge-on half way: {}",
        normal(0.5)
    );
    assert!(
        normal(0.25).z > 0.5 && normal(0.75).z > 0.5,
        "always the upper side"
    );
}
