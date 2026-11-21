//! Script timing and beats laid out in time.

#![expect(
    clippy::float_cmp,
    reason = "a beat's times are sums of whole milliseconds, which are exact"
)]

use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::{Act, Curve as Named, Room, Step, StepTiming, Subject};
use game_play::beat::{Laid, Opens, beat};
use game_play::named_curve;
use game_probe::testing::object;
use kit_base::pose::Curve;

#[test]
fn named_script_curves_use_the_corresponding_pose_curves() {
    assert_eq!(named_curve(Named::Linear), Curve::LINEAR);
    assert_eq!(named_curve(Named::EaseIn), Curve::EASE_IN);
    assert_eq!(named_curve(Named::EaseOut), Curve::EASE_OUT);
    assert_eq!(named_curve(Named::EaseInOut), Curve::EASE_IN_OUT);
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

/// A step the client puts first in a beat, a closed prompt's tray letting its cards go, is laid
/// out as though it were the beat's first step, as the web's `lowered` puts it there: the
/// session's steps take their turns after it and the beat lasts until it is over.
#[test]
fn a_leading_step_is_laid_out_first_in_its_beat() {
    let timings = [
        timing(|t| t.duration = 33.0),
        timing(|t| (t.delay, t.duration) = (20.0, 50.0)),
    ];
    for lead in [
        timing(|_| {}),
        timing(|t| (t.delay, t.stagger) = (40.0, 100.0)),
    ] {
        let steps = [tap(1), tap(2)];
        let mut led = vec![Laid::default(); steps.len()];
        let end = game_play::beat::lay(
            &steps,
            &timings,
            500.0,
            |_| None,
            (500.0, Some(&lead)),
            &mut led,
        );
        let (whole, whole_end) = lay(
            &[9, 1, 2],
            &[lead.clone(), timings[0].clone(), timings[1].clone()],
            500.0,
            &[],
            500.0,
        );
        assert_eq!((&led[..], end), (&whole[1..], whole_end), "{lead:?}");
    }
    let (_, without) = lay(&[1, 2], &timings, 500.0, &[], 500.0);
    let mut led = vec![Laid::default(); 2];
    let with = game_play::beat::lay(
        &[tap(1), tap(2)],
        &timings,
        500.0,
        |_| None,
        (500.0, Some(&timing(|_| {}))),
        &mut led,
    );
    assert_eq!(
        (without, with),
        (570.0, 800.0),
        "the beat lasts until the 300 ms let-go is over"
    );
}
