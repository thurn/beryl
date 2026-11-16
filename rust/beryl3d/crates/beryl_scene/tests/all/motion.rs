//! Poses from segments, tracks of segments, and beats laid out in time.

#![expect(
    clippy::float_cmp,
    reason = "a beat's times are sums of whole milliseconds, which are exact"
)]

use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::{Act, Curve as Named, Room, Step, StepTiming, Subject};
use beryl_scene::beat::{Laid, Opens, beat};
use beryl_scene::motion::{Full, SEGMENTS, Track, pose_at};
use beryl_scene::pose::{Curve, Pose, Segment};
use beryl_scene::testing::object;
use beryl_scene::verbs::Interval;
use glam::{Quat, Vec3};

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
    segments[0].curve = Curve::named(Named::EaseIn);
    assert!(close(
        pose_at(&segments, 100 * MS).position.x,
        100.0 - 68.464_32
    ));
    segments[0].curve = Curve::named(Named::Linear);
    assert!(close(pose_at(&segments, 50 * MS).position.x, 25.0));
    segments[0].curve = Curve::named(Named::EaseInOut);
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

fn tap(slot: u16) -> Step {
    let mut object = object(
        ObjectId(slot),
        ZoneName::Battlefield,
        Side::You,
        Some("Card"),
    );
    object.tapped = true;
    Step {
        act: Act::Tap { object },
        cause: None,
    }
}

fn thing(slot: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(slot),
    }
}

/// A timing of 300 ms with no delay or stagger, changed by `f`.
fn timing(f: impl FnOnce(&mut StepTiming)) -> StepTiming {
    let mut timing = StepTiming {
        case: None,
        duration: 300.0,
        delay: 0.0,
        stagger: 0.0,
        curve: Named::Linear,
        each: None,
        with: false,
        after: Vec::new(),
        hold: None,
        leads: false,
        passes: false,
        alone: false,
        opens: false,
        ahead: false,
        closes: None,
        room: Room::Step,
        phases: Vec::new(),
        effects: Vec::new(),
    };
    f(&mut timing);
    timing
}

/// A step laid out at `turn`, from `from` to `to`, with its act applied at `opens.0` and
/// room made by `opens.1`.
const fn laid(turn: f64, from: f64, to: f64, opens: Option<(f64, f64)>) -> Laid {
    Laid {
        turn,
        from,
        to,
        opens: match opens {
            Some((at, by)) => Some(Opens { at, by }),
            None => None,
        },
    }
}

/// Lays out a beat of taps on `slots` with `timings`, from `at`, `whole` giving since when the
/// span the step on each slot ends has stood whole, and `passed`.
fn lay(
    slots: &[u16],
    timings: &[StepTiming],
    at: f64,
    whole: &[(u16, f64)],
    passed: f64,
) -> (Vec<Laid>, f64) {
    let steps: Vec<Step> = slots.iter().map(|&slot| tap(slot)).collect();
    let whole = |act: &Act| {
        let Act::Tap { object } = act else {
            return None;
        };
        whole
            .iter()
            .find(|(slot, _)| ObjectId(*slot) == object.slot)
            .map(|&(_, since)| since)
    };
    let mut laid = Vec::new();
    let end = beat(&steps, timings, at, whole, passed, &mut laid);
    (laid, end)
}

// The expected values below are what the web interface's `laid` (ui/src/stage/beat.ts) gives
// for the same five beats: each step a `tap` of the slot given, with a `Timing` of the same
// values and `whole` and `passed` as here. They were worked out once in Node, from ui/ with
// node_modules restored, by importing `laid` from ../src/stage/beat.ts, `tableOf` from
// ../src/stage/table.ts and `view` and `object` from ./stage-table.mjs, and calling
// `laid(steps, 0, at, tableOf(view([1, 2, 3, 4, 9])), () => timings[i++], whole, passed)`.

/// Turns and staggers, a step played `with` the one before it, and one that leads. The step
/// that leads has its act applied as it begins, so it reports no `opens`, though what follows
/// it may begin only once it is over.
#[test]
fn a_beat_takes_its_steps_in_turn() {
    let (steps, end) = lay(
        &[1, 2, 3, 4],
        &[
            timing(|t| (t.delay, t.stagger) = (50.0, 100.0)),
            timing(|t| (t.delay, t.stagger, t.with) = (30.0, 100.0, true)),
            timing(|t| (t.delay, t.stagger, t.leads, t.duration) = (10.0, 80.0, true, 250.0)),
            timing(|t| t.duration = 200.0),
        ],
        1000.0,
        &[],
        1000.0,
    );
    assert_eq!(end, 1560.0);
    assert_eq!(
        steps,
        [
            laid(1000.0, 1050.0, 1350.0, None),
            laid(1000.0, 1080.0, 1380.0, None),
            laid(1100.0, 1110.0, 1360.0, None),
            laid(1180.0, 1360.0, 1560.0, None),
        ]
    );
}

/// A step drawn after what another moves waits for it, one waiting on what nothing in the
/// beat moves waits for nothing, and one that passes over the table does not hold the beat.
#[test]
fn a_beat_waits_for_what_a_step_lies_between() {
    let (steps, end) = lay(
        &[1, 2, 3, 4],
        &[
            timing(|t| (t.duration, t.stagger) = (400.0, 50.0)),
            timing(|t| (t.after, t.delay, t.duration) = (vec![thing(1)], 20.0, 100.0)),
            timing(|t| (t.passes, t.duration, t.stagger) = (true, 2000.0, 10.0)),
            timing(|t| (t.after, t.duration) = (vec![thing(9)], 50.0)),
        ],
        0.0,
        &[],
        0.0,
    );
    assert_eq!(end, 520.0);
    assert_eq!(
        steps,
        [
            laid(0.0, 0.0, 400.0, None),
            laid(50.0, 420.0, 520.0, None),
            laid(50.0, 420.0, 2420.0, None),
            laid(60.0, 420.0, 470.0, None),
        ]
    );
}

/// A step that ends a span waits for it to have stood whole for its hold, and one that has
/// the table to itself waits until nothing passes over it.
#[test]
fn a_beat_holds_spans_and_waits_for_the_table() {
    let (steps, end) = lay(
        &[1, 2, 3],
        &[
            timing(|t| (t.hold, t.duration, t.stagger) = (Some(500.0), 200.0, 60.0)),
            timing(|t| (t.hold, t.duration) = (Some(100.0), 100.0)),
            timing(|t| (t.alone, t.delay, t.duration) = (true, 40.0, 120.0)),
        ],
        1200.0,
        &[(1, 1000.0), (2, 1500.0)],
        2000.0,
    );
    assert_eq!(end, 2160.0);
    assert_eq!(
        steps,
        [
            laid(1200.0, 1500.0, 1700.0, None),
            laid(1260.0, 1600.0, 1700.0, None),
            laid(1260.0, 2040.0, 2160.0, None),
        ]
    );
}

/// Steps that go ahead have their acts applied together, as the first of them begins, and
/// one the table opens for has its act applied as its delay begins.
#[test]
fn a_beat_applies_acts_ahead_of_their_steps() {
    let ahead = || timing(|t| (t.ahead, t.delay, t.stagger) = (true, 100.0, 150.0));
    let (steps, end) = lay(
        &[1, 2, 3, 4],
        &[
            ahead(),
            ahead(),
            ahead(),
            timing(|t| (t.opens, t.delay, t.duration) = (true, 200.0, 100.0)),
        ],
        500.0,
        &[],
        500.0,
    );
    assert_eq!(end, 1250.0);
    assert_eq!(
        steps,
        [
            laid(500.0, 600.0, 900.0, None),
            laid(650.0, 750.0, 1050.0, Some((600.0, 750.0))),
            laid(800.0, 900.0, 1200.0, Some((600.0, 750.0))),
            laid(950.0, 1150.0, 1250.0, Some((950.0, 1150.0))),
        ]
    );
}

/// Every rule at once, in one beat.
#[test]
fn a_beat_combines_its_rules() {
    let (steps, end) = lay(
        &[1, 2, 3, 1, 4],
        &[
            timing(|t| (t.stagger, t.duration, t.passes) = (120.0, 500.0, true)),
            timing(|t| {
                (t.with, t.delay, t.leads, t.duration, t.stagger) = (true, 70.0, true, 90.0, 40.0);
            }),
            timing(|t| {
                (t.hold, t.opens, t.delay, t.duration, t.stagger) =
                    (Some(250.0), true, 30.0, 60.0, 20.0);
            }),
            timing(|t| {
                (t.after, t.ahead, t.delay, t.duration, t.alone) =
                    (vec![thing(2), thing(3)], true, 5.0, 10.0, true);
            }),
            timing(|t| (t.with, t.opens, t.delay, t.duration) = (true, true, 15.0, 0.0)),
        ],
        300.0,
        &[(3, 0.0)],
        900.0,
    );
    assert_eq!(end, 920.0);
    assert_eq!(
        steps,
        [
            laid(300.0, 300.0, 800.0, None),
            laid(300.0, 370.0, 460.0, None),
            laid(420.0, 460.0, 520.0, Some((420.0, 460.0))),
            laid(440.0, 905.0, 915.0, None),
            laid(440.0, 920.0, 920.0, Some((905.0, 920.0))),
        ]
    );
}

/// What is drawn on a card (its plaque, its marks) lies on the side of the card that faces up:
/// flat on its face while face up and on its back while face down, and edge-on with the card
/// half way through turning over.
#[test]
fn what_is_drawn_on_a_card_turns_over_with_it() {
    let normal = |turn: f32| {
        let pose = beryl_scene::pose::Pose {
            turn,
            ..beryl_scene::pose::Pose::default()
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
