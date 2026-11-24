//! The segment and step checks, the check times and checking a whole recording
//! (docs/beryl-3d.md, "Interface checks" and "Test speed"), on the three-beat script of the
//! script clock's tests, played by the test driver.

use crate::common::{MS, before, emerge, resolve, three_beats};
use app_client::driver::{Checked, Mode, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Side, Why, ZoneName};
use beryl_protocol::script::Verb;
use game_play::play::record::{Cause, MoveKey, Planted, ScriptRecord, SegmentRecord, StepRecord};
use game_play::verbs::Interval;
use game_probe::agent::steps::{crowded, no_jumps, rests_in_place};
use game_table::table::Table;
use glam::Vec3;
use kit_base::pose::{Curve, Pose, Segment};
use kit_probe::agent::{Context, Fault, Rule};
use kit_ui::input::{Insets, Size};

const SIZE: Size = Size {
    width: 1440.0,
    height: 900.0,
};

/// The table before, then the three-beat script with its timings: the Plains flies from the hand
/// to the battlefield, a Soldier token appears, and the Angel's sickness wears off as the Island
/// taps. The script starts 400 ms in, when the recorded reply to the first view is given.
fn page() -> Page {
    page_with_flight_duration(420.0)
}

fn page_with_flight_duration(duration: f64) -> Page {
    let (start, cards) = before();
    let (mut after, mut timings) = three_beats(&start, cards);
    timings[0].duration = duration;
    after
        .view
        .script
        .as_mut()
        .expect("the three-beat view has a script")
        .timing = timings;
    let mut messages = vec![
        FixtureMessage::game_update(&start),
        FixtureMessage::game_update(&after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let recording = Recording(messages);
    Page::new(crate::test_data::data(), recording.into(), Side::You, SIZE)
        .expect("the views can be read")
        .named("three-beats")
}

/// When the Plains is in the middle of its flight: 210 ms into the script.
const MID_FLIGHT: u64 = 610 * MS;

/// The faults `rule` found.
fn of(checked: &Checked, rule: Rule) -> Vec<&Fault> {
    checked
        .faults
        .iter()
        .filter(|fault| fault.rule == rule)
        .collect()
}

fn printed(fault: &Fault) -> String {
    game_probe::agent::print_fault(
        fault,
        "three-beats",
        Side::You,
        &Context {
            size_class: crate::REGULAR,
            audit: crate::test_data::data().audit,
            checks: crate::test_data::data()
                .profiles
                .for_size_class(crate::REGULAR)
                .checks,
            size: SIZE,
            safe_area: Insets::default(),
            at: fault.at,
        },
    )
}

/// The three-beat script with `kind` planted at `at`, checked at the check times.
fn planted(kind: Planted, at: u64) -> Checked {
    let mut page = page();
    page.plant(kind, at).expect("a script plays then");
    page.check_all(Mode::AtCheckTimes)
}

/// Each fault of `checked`, as the rule and the elements it names.
fn keys(checked: &Checked) -> Vec<(Rule, Vec<String>)> {
    checked
        .faults
        .iter()
        .map(|fault| {
            (
                fault.rule,
                fault.elements.iter().map(|e| e.locator.clone()).collect(),
            )
        })
        .collect()
}

#[test]
fn the_three_beat_script_has_no_fault() {
    let checked = page().check_all(Mode::AtCheckTimes);
    let printed: Vec<String> = checked.faults.iter().map(printed).collect();
    assert!(printed.is_empty(), "{}", printed.join("\n"));
}

#[test]
fn extending_a_flight_adds_visibility_samples_with_the_same_audit_work() {
    let short = page().check_all(Mode::AtCheckTimes);
    let mut long = page_with_flight_duration(1_429.0);
    let checked = long.check_all(Mode::AtCheckTimes);
    assert!(checked.visibility_only_frames > short.visibility_only_frames);
    assert_eq!(
        checked.frames - checked.visibility_only_frames,
        short.frames - short.visibility_only_frames
    );

    long.plant(Planted::Unseen, MID_FLIGHT)
        .expect("a flight plays then");
    let sparse = long.check_all(Mode::AtCheckTimes);
    let every = long.check_all(Mode::EveryTick);
    assert_eq!(of(&sparse, Rule::StepShown).len(), 1);
    assert_eq!(of(&sparse, Rule::StepShown), of(&every, Rule::StepShown));
}

#[test]
fn the_check_times_hold_each_segment_and_step_edge() {
    let times = page().check_times();
    // The script starts at 400 ms; the flight lasts 420 ms, so its start, apex and end.
    for t in [400, 610, 820] {
        assert!(
            times.contains(&(t * MS)),
            "{t} ms is a check time: {times:?}"
        );
    }
    // The token's appearance ends 300 ms after the flight, and the tap 150 ms after that.
    for t in [1120, 1270] {
        assert!(
            times.contains(&(t * MS)),
            "{t} ms is a check time: {times:?}"
        );
    }
    assert!(
        times.windows(2).all(|pair| pair[0] < pair[1]),
        "in order, each once"
    );
}

#[test]
fn a_planted_jump_fails_no_jumps() {
    let checked = planted(Planted::Jump, MID_FLIGHT);
    let jumps = of(&checked, Rule::Jump);
    assert!(!jumps.is_empty(), "the jump is found: {:?}", checked.faults);
    let jump = jumps[0];
    assert!(
        jump.elements[0].locator.contains("Plains"),
        "it names the card that jumped: {}",
        printed(jump)
    );
    assert_eq!(jump.at, MID_FLIGHT, "it names the moment of the jump");
    assert!(printed(jump).contains("t=610ms"), "{}", printed(jump));
}

#[test]
fn an_object_left_off_its_placement_fails_rest_equals_placement() {
    let checked = planted(Planted::OffPlacement, MID_FLIGHT);
    let faults = of(&checked, Rule::RestPlacement);
    assert_eq!(faults.len(), 1, "{:?}", checked.faults);
    assert!(
        faults[0].message.contains("Plains"),
        "{}",
        printed(faults[0])
    );
    // The script ends 870 ms after it starts.
    assert_eq!(faults[0].at, 1270 * MS, "{}", printed(faults[0]));
}

#[test]
fn a_verb_played_short_fails_the_step_time() {
    let checked = planted(Planted::Duration, MID_FLIGHT);
    let faults = of(&checked, Rule::StepTime);
    assert_eq!(faults.len(), 1, "{:?}", checked.faults);
    assert!(
        faults[0]
            .message
            .contains("moved for 210 ms in step:2.0.0 (fly), which lasts 420 ms"),
        "{}",
        printed(faults[0])
    );
}

#[test]
fn a_step_whose_subject_is_outside_the_window_fails_every_step_shown() {
    let checked = planted(Planted::Unseen, MID_FLIGHT);
    let faults = of(&checked, Rule::StepShown);
    assert_eq!(faults.len(), 1, "{:?}", checked.faults);
    let fault = faults[0];
    assert!(
        fault.message.contains("step:2.0.0 (fly)"),
        "{}",
        printed(fault)
    );
    assert_eq!(
        fault.at,
        400 * MS,
        "the moment the step begins: {}",
        printed(fault)
    );
}

#[test]
fn four_moves_beginning_at_once_fail_not_crowded() {
    let checked = planted(Planted::Crowded, MID_FLIGHT);
    let faults = of(&checked, Rule::Crowded);
    assert_eq!(faults.len(), 1, "{:?}", checked.faults);
    assert!(
        faults[0].message.contains("beat 1"),
        "{}",
        printed(faults[0])
    );
    assert_eq!(faults[0].at, 400 * MS);
}

/// A step of the first beat that starts at `from` milliseconds and flies a card in from `zone`.
fn flight(from: u64, zone: ZoneName, why: Why) -> StepRecord {
    StepRecord {
        view: 1,
        beat: 0,
        index: 0,
        verb: Verb::Fly,
        subject: None,
        from: from * MS,
        to: (from + 400) * MS,
        timed: 400 * MS,
        key: Some(MoveKey {
            verb: Verb::Fly,
            from: Some(zone),
            to: Some(ZoneName::Battlefield),
            why: Some(why),
        }),
        cues: [None, None],
    }
}

/// A script that started at 0, with `steps` and `segments`.
fn script(steps: Vec<StepRecord>, segments: Vec<SegmentRecord>) -> ScriptRecord {
    ScriptRecord {
        start: Some(0),
        rate: 1,
        end: 1000 * MS,
        steps,
        segments,
        finals: Vec::new(),
        effects: Vec::new(),
    }
}

#[test]
fn moves_of_one_kind_together_are_not_crowded_and_four_kinds_are() {
    let same: Vec<StepRecord> = (0..4)
        .map(|_| flight(0, ZoneName::Hand, Why::Play))
        .collect();
    assert!(
        crowded(&script(same, Vec::new()), crate::test_data::data().audit).is_empty(),
        "four of one move read as one"
    );
    let mixed = vec![
        flight(0, ZoneName::Hand, Why::Play),
        flight(20, ZoneName::Library, Why::Play),
        flight(40, ZoneName::Graveyard, Why::Play),
        flight(60, ZoneName::Exile, Why::Play),
    ];
    assert_eq!(
        crowded(
            &script(mixed.clone(), Vec::new()),
            crate::test_data::data().audit
        )
        .len(),
        1
    );
    let mut apart = mixed;
    apart[3].from = 200 * MS;
    assert!(
        crowded(&script(apart, Vec::new()), crate::test_data::data().audit).is_empty(),
        "the fourth begins 200 ms later"
    );
}

#[test]
fn two_segments_that_do_not_meet_fail_no_jumps() {
    let pose = |x: f32| Pose {
        position: Vec3::new(x, 0.0, 0.0),
        ..Pose::default()
    };
    let segment = |start: u64, from: f32, to: f32| {
        Segment::between(
            pose(from),
            pose(to),
            Interval {
                from: start * MS,
                to: (start + 200) * MS,
            },
            Curve::LINEAR,
        )
    };
    let steps = vec![
        flight(0, ZoneName::Hand, Why::Play),
        flight(200, ZoneName::Hand, Why::Play),
    ];
    let record = |segment, step| SegmentRecord {
        slot: 3,
        cause: Cause::Verb(step),
        segment,
    };
    let meeting = script(
        steps.clone(),
        vec![
            record(segment(0, 0.0, 50.0), 0),
            record(segment(200, 50.0, 90.0), 1),
        ],
    );
    assert!(no_jumps(&meeting).is_empty());
    let apart = script(
        steps,
        vec![
            record(segment(0, 0.0, 50.0), 0),
            record(segment(200, 60.0, 90.0), 1),
        ],
    );
    let found = no_jumps(&apart);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].slots, vec![3]);
    assert_eq!(found[0].at, 200 * MS);
    assert!(
        found[0]
            .message
            .contains("jumped 10 mm as step:1.0.0 (fly) began"),
        "{found:?}"
    );
}

#[test]
fn the_check_times_find_every_fault_that_every_tick_finds() {
    let mut frames = None;
    for kind in [
        Planted::Jump,
        Planted::OffPlacement,
        Planted::Duration,
        Planted::Unseen,
        Planted::Crowded,
    ] {
        for at in [450, MID_FLIGHT / MS, 900, 1200] {
            let mut page = page();
            page.plant(kind, at * MS).expect("a script plays then");
            let sparse = page.check_all(Mode::AtCheckTimes);
            let every = page.check_all(Mode::EveryTick);
            let found = keys(&sparse);
            for key in keys(&every) {
                assert!(
                    found.contains(&key),
                    "{kind:?} at {at} ms: every tick finds {key:?}, the check times do not: {:?}",
                    sparse.faults
                );
            }
            frames.get_or_insert((sparse.frames, every.frames));
        }
    }
    let (sparse, every) = frames.expect("checked");
    eprintln!(
        "frames evaluated on the three-beat script: {sparse} at the check times, {every} on every tick"
    );
}

#[test]
fn an_ability_that_goes_on_the_stack_and_resolves_has_no_fault() {
    // Rest equals placement and no duplicates hold for the ability's card at every check time.
    let (start, cards) = before();
    let (mut emerged, timings) = emerge(&start, cards, 5);
    emerged.view.script.as_mut().expect("a script").timing = timings;
    let (mut resolved, timings) = resolve(&emerged, 5);
    resolved.view.script.as_mut().expect("a script").timing = timings;
    let mut messages = vec![
        FixtureMessage::game_update(&start),
        FixtureMessage::game_update(&emerged),
        FixtureMessage::game_update(&resolved),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        SIZE,
    )
    .expect("the views can be read")
    .named("abilities");
    let checked = page.check_all(Mode::AtCheckTimes);
    let printed: Vec<String> = checked.faults.iter().map(printed).collect();
    assert!(printed.is_empty(), "{}", printed.join("\n"));
}

#[test]
fn a_flight_back_to_where_it_started_rests_in_place() {
    let pose = |x: f32| Pose {
        position: Vec3::new(x, 0.0, 0.0),
        ..Pose::default()
    };
    let leg = |start: u64, from: f32, to: f32| SegmentRecord {
        slot: 3,
        cause: Cause::Verb(0),
        segment: Segment::between(
            pose(from),
            pose(to),
            Interval {
                from: start * MS,
                to: (start + 200) * MS,
            },
            Curve::LINEAR,
        ),
    };
    let step = StepRecord {
        subject: Some(3),
        ..flight(0, ZoneName::Graveyard, Why::Exile)
    };
    let table = Table::new(
        crate::test_data::data(),
        game_table::table::Window::default(),
    );
    let camera = table.camera();
    let there_and_back = script(
        vec![step.clone()],
        vec![leg(0, 0.0, 50.0), leg(200, 50.0, 0.0)],
    );
    assert!(rests_in_place(
        &there_and_back,
        0,
        camera,
        crate::test_data::data().audit
    ));
    let away = script(vec![step], vec![leg(0, 0.0, 50.0)]);
    assert!(!rests_in_place(
        &away,
        0,
        camera,
        crate::test_data::data().audit
    ));
}
