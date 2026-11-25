//! The frame log's writer and reader agree: what a run writes reads back as the same run, the
//! same late frames and the same summary.

use kit_ui::input::Input as DeviceInput;
use std::path::PathBuf;

use app_client::Input;
use beryl_client::frame_log::{self, FrameLog, InputLine, Run};
use beryl_client::frame_trace::Reason;
use beryl_host::rate::Step;
use beryl_host::timing::{DisplayTick, FrameTiming, GraphicsTier, Outcome, PacingChange, Timing};
use beryl_host::{Shown, Ticked};
use kit_ui::input::{Insets, Lifecycle, Size};
use std::time::Instant;

const MS: u64 = 1_000_000;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-client-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn ticked(tick_ms: u64, since_last_ms: Option<u64>, shown: bool) -> Ticked {
    Ticked {
        tick: tick_ms * MS,
        clock: tick_ms * MS,
        since_last: since_last_ms.map(|gap| gap * MS),
        client: MS / 10,
        shown: shown.then_some(Shown {
            waited: 2 * MS,
            encoded: MS / 2,
        }),
        hz: 120.0,
        step: None,
    }
}

#[test]
fn what_the_writer_writes_the_reader_reads() {
    let run = Run {
        checkout: "/checkout".to_owned(),
        pid: 42,
        started_ms: 1_000,
        window: "background".to_owned(),
        display_hz: 120.0,
        adapter: "a GPU".to_owned(),
        load: Some([20.0, 30.0, 40.0]),
        fixture: Some("verbs-zones".to_owned()),
        pause_at_ms: Some(100.0),
        drive: Some("hover-hand".to_owned()),
        load_threads: Some("16:utility".to_owned()),
        slow_gpu_ms: Some(5.0),
        game: Some("expert against mcts-hidden".to_owned()),
    };
    let dir = dir("frame-log");
    let mut log = FrameLog::create(&dir, &run).expect("the log is made");
    // A first frame, one on time, one held to 60 Hz, one that missed 60 Hz, one with no
    // drawable, and two at or after the pause.
    log.frame(&ticked(10, None, true), &[], false);
    log.frame(&ticked(18, Some(8), true), &[], false);
    log.frame(&ticked(35, Some(17), true), &["fly", "set"], true);
    log.frame(&ticked(75, Some(40), true), &["appear"], false);
    log.frame(&ticked(83, Some(8), false), &[], false);
    log.frame(&ticked(100, Some(17), true), &[], false);
    log.frame(&ticked(200, None, true), &[], false);
    log.unbuilt("tap");
    let path = log.path().to_owned();
    let summary = log.finish().expect("the log is finished");

    let read = frame_log::read(&path).expect("the log reads back");
    assert_eq!(read.run, run);
    assert!(read.timing_windows.is_empty());
    assert!(read.pacing_changes.is_empty());
    assert_eq!(read.summary.as_ref(), Some(&summary));
    assert_eq!(summary.frames, 7);
    assert_eq!(summary.skipped, 1);
    assert_eq!(
        (summary.late, summary.held_to_60, summary.missed_60),
        (3, 2, 1)
    );
    assert_eq!(summary.after_pause, 1);
    assert_eq!(summary.unbuilt, ["tap"]);
    // Only the frame at 35 ms was drawn while the AI searched, and it came late.
    assert_eq!((summary.searching.frames, summary.searching.late), (1, 1));
    let gaps: Vec<f64> = read.late.iter().map(|late| late.since_last_ms).collect();
    assert_eq!(gaps, [17.0, 40.0, 17.0]);
    assert_eq!(read.late[0].verbs, ["fly", "set"]);
    assert!(read.late[0].searching && read.late[0].held_to_60);
    assert!(!read.late[1].held_to_60);
    let printed = summary.print(&read.run);
    assert!(
        printed.contains(
            "drive hover-hand, load threads 16:utility, slow GPU 5 ms, game expert against \
             mcts-hidden"
        ),
        "{printed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn frames_are_late_against_the_rate_the_host_chose() {
    let run = Run {
        checkout: "/checkout".to_owned(),
        pid: 44,
        started_ms: 3_000,
        window: "fullscreen".to_owned(),
        display_hz: 120.0,
        adapter: "a GPU".to_owned(),
        load: None,
        fixture: None,
        pause_at_ms: None,
        drive: None,
        load_threads: None,
        slow_gpu_ms: None,
        game: None,
    };
    let dir = dir("rate");
    let mut log = FrameLog::create(&dir, &run).expect("the log is made");
    // At 120 Hz a frame 17 ms after the last is held to 60 Hz, a fault. After the host steps
    // down to 60 Hz, the same gap is on time and a 30 ms one is late.
    log.frame(&ticked(10, None, true), &[], false);
    log.frame(&ticked(27, Some(17), true), &[], false);
    let mut stepped = ticked(44, Some(17), true);
    stepped.step = Some(Step::Rate(60.0));
    log.frame(&stepped, &[], false);
    for (tick, gap) in [(61, 17), (78, 17), (108, 30)] {
        let mut frame = ticked(tick, Some(gap), true);
        frame.hz = 60.0;
        log.frame(&frame, &[], false);
    }
    let path = log.path().to_owned();
    let summary = log.finish().expect("the log is finished");
    let read = frame_log::read(&path).expect("the log reads back");
    assert_eq!(
        (summary.late, summary.held_to_60, summary.missed_60),
        (3, 2, 1)
    );
    let rates: Vec<f32> = read.late.iter().map(|late| late.hz).collect();
    assert_eq!(rates, [120.0, 120.0, 60.0]);
    assert!((summary.hz - 60.0).abs() < f32::EPSILON);
    assert_eq!(summary.steps.len(), 1);
    assert_eq!(summary.steps[0].hz, Some(60.0));
    let printed = summary.print(&read.run);
    assert!(
        printed.contains("asked for 120 Hz, then 60 Hz at 44 ms; 60 Hz at the end"),
        "{printed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_summary_gives_the_median_and_95th_percentile_wait_for_a_drawable() {
    let run = Run {
        checkout: "/checkout".to_owned(),
        pid: 43,
        started_ms: 2_000,
        window: "none".to_owned(),
        display_hz: 60.0,
        adapter: "a GPU".to_owned(),
        load: None,
        fixture: None,
        pause_at_ms: None,
        drive: None,
        load_threads: None,
        slow_gpu_ms: None,
        game: None,
    };
    let dir = dir("waits");
    let mut log = FrameLog::create(&dir, &run).expect("the log is made");
    // Twenty frames that waited 1, 2, ..., 20 ms, in a shuffled order.
    for (index, wait) in [
        7, 20, 1, 13, 4, 18, 10, 2, 16, 5, 11, 19, 3, 14, 8, 17, 6, 12, 9, 15,
    ]
    .into_iter()
    .enumerate()
    {
        let mut frame = ticked(index as u64 * 16, None, true);
        frame.shown = Some(Shown {
            waited: wait * MS,
            encoded: 0,
        });
        log.frame(&frame, &[], false);
    }
    let summary = log.finish().expect("the log is finished");
    // Waits are counted to a tenth of a millisecond, each at the top of its tenth.
    let near = |value: f64, expected: f64| (value - expected).abs() < 0.11;
    assert!(near(summary.median_wait_ms, 10.0), "{summary:?}");
    assert!(near(summary.p95_wait_ms, 19.0), "{summary:?}");
    assert!(near(summary.worst_wait_ms, 20.0), "{summary:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The log's thread creates the file, so a folder that cannot be made does not stop the run;
/// `finish` reports it.
#[test]
fn a_log_that_cannot_be_written_is_reported_when_it_finishes() {
    let dir = dir("frame-log-blocked");
    std::fs::write(&dir, b"a file where the folder should be").expect("the file is written");
    let run = Run {
        checkout: "/checkout".to_owned(),
        pid: 43,
        started_ms: 2_000,
        window: "none".to_owned(),
        display_hz: 60.0,
        adapter: "a GPU".to_owned(),
        load: None,
        fixture: None,
        pause_at_ms: None,
        drive: None,
        load_threads: None,
        slow_gpu_ms: None,
        game: None,
    };
    let mut log = FrameLog::create(&dir, &run).expect("the log's thread starts");
    log.frame(&ticked(10, None, true), &[], false);
    assert!(log.finish().is_err(), "a log in no folder finished");
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn an_input_line_says_how_long_ago_the_last_frame_was_drawn() {
    let run = Run {
        checkout: "/checkout".to_owned(),
        pid: 45,
        started_ms: 4_000,
        window: "phone".to_owned(),
        display_hz: 60.0,
        adapter: "a GPU".to_owned(),
        load: None,
        fixture: None,
        pause_at_ms: None,
        drive: None,
        load_threads: None,
        slow_gpu_ms: None,
        game: None,
    };
    let dir = dir("inputs");
    let mut log = FrameLog::create(&dir, &run).expect("the log is made");
    let surface = InputLine::Surface {
        width: 393.0,
        height: 852.0,
        scale: 3.0,
        top: 59.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    };
    log.input(&Input::Device(DeviceInput::Surface {
        size: Size {
            width: 393.0,
            height: 852.0,
        },
        scale: 3.0,
        safe_area: Insets {
            top: 59.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        },
    }));
    log.frame(&ticked(10, None, true), &[], false);
    let lifecycle = InputLine::Lifecycle {
        event: "Suspended".to_owned(),
    };
    log.input(&Input::Device(DeviceInput::Lifecycle(Lifecycle::Suspended)));
    let path = log.path().to_owned();
    log.finish().expect("the log is finished");

    let read = frame_log::read(&path).expect("the log reads back");
    assert_eq!(read.inputs, [surface, lifecycle]);
    // The surface came before any frame, so its line has no time since one.
    let text = std::fs::read_to_string(&path).expect("the log reads");
    let inputs: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .filter_map(|line: serde_json::Value| line.get("input").cloned())
        .collect();
    assert_eq!(inputs[0].get("since_frame_ms"), None);
    assert_eq!(inputs[0]["surface"]["top"], 59.0);
    let since = inputs[1]["since_frame_ms"]
        .as_f64()
        .expect("a time since the frame");
    assert!(since >= 0.0, "{since}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn plain_run(pid: u32) -> Run {
    Run {
        checkout: "/checkout".to_owned(),
        pid,
        started_ms: 5_000,
        window: "fullscreen".to_owned(),
        display_hz: 120.0,
        adapter: "a GPU".to_owned(),
        load: None,
        fixture: None,
        pause_at_ms: None,
        drive: None,
        load_threads: None,
        slow_gpu_ms: None,
        game: None,
    }
}

/// The host's timing record of the tick at `at_ms`, as the desktop host notes it after the
/// frame log has counted the frame.
fn record(sequence: u64, at_ms: u64) -> Timing {
    let at = at_ms * MS;
    Timing::Frame(FrameTiming {
        tick: DisplayTick {
            sequence,
            epoch: 1,
            requested_hz: 120.0,
            callback_ns: at,
            display_ns: Some(at as i64),
            target_ns: Some(at as i64 + 8 * MS as i64),
            reported_hz: Some(120.0),
        },
        handled_ns: at + MS / 10,
        finished_ns: at + MS,
        outcome: Outcome::Submitted,
        submitted_ns: Some(at + MS / 2),
        cpu: None,
        render_size: [1024, 768],
        fullscreen: true,
        tier: GraphicsTier::Baseline,
        animating: true,
        searching: false,
        gpu_in_flight: None,
        presented_ns: None,
    })
}

/// Draws the frame at `at_ms`, `gap_ms` after the one before, and notes its timing record.
fn draw(log: &mut FrameLog, sequence: u64, at_ms: u64, gap_ms: u64) {
    log.frame(&ticked(at_ms, Some(gap_ms), true), &["fly"], false);
    log.timing(&record(sequence, at_ms));
}

fn lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("the log reads")
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect()
}

/// macOS ends the process after winit's `exiting`, so the summary written there must already
/// be on disk when `exit` returns, and a later `finish` writes no second one.
#[test]
fn a_log_ended_through_exiting_has_its_summary_on_disk() {
    let dir = dir("exiting");
    let mut log = FrameLog::create(&dir, &plain_run(46)).expect("the log is made");
    log.frame(&ticked(10, None, true), &[], false);
    log.frame(&ticked(30, Some(20), true), &[], false);
    log.exit();
    let path = log.path().to_owned();
    let read = frame_log::read(&path).expect("the log reads back");
    let summary = read.summary.expect("a summary before the process ends");
    assert_eq!((summary.frames, summary.late), (2, 1));
    assert_eq!(summary.rates.len(), 1);
    let rate = &summary.rates[0];
    assert_eq!((rate.hz, rate.frames, rate.late), (120.0, 2, 1));
    assert!((rate.seconds - 0.02).abs() < 1e-9, "{rate:?}");
    let finished = log.finish().expect("the log is finished");
    assert_eq!(finished, summary);
    let summaries = lines(&path)
        .iter()
        .filter(|line| line.get("summary").is_some())
        .count();
    assert_eq!(summaries, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the trace on, a late frame's line still is written, with its timing record and the
/// one before it.
#[test]
fn a_late_frame_with_the_trace_on_has_its_timing_records() {
    let dir = dir("late-trace");
    let mut log = FrameLog::create(&dir, &plain_run(47)).expect("the log is made");
    log.start_trace(Instant::now());
    log.frame(&ticked(10, None, true), &[], false);
    log.timing(&record(1, 10));
    draw(&mut log, 2, 30, 20);
    let path = log.path().to_owned();
    log.finish().expect("the log is finished");
    let read = frame_log::read(&path).expect("the log reads back");
    assert_eq!(read.late.len(), 1);
    let late = &read.late[0];
    assert_eq!(late.verbs, ["fly"]);
    let sequence = |timing: Option<FrameTiming>| timing.map(|frame| frame.tick.sequence);
    assert_eq!(sequence(late.frame), Some(2));
    assert_eq!(sequence(late.previous), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Three late frames within a second capture one `late` window; another burst inside the
/// 30 s cool-down captures none, but a change of rate still captures a `step` window.
#[test]
fn bursts_of_late_frames_and_steps_capture_windows() {
    let dir = dir("late-windows");
    let mut log = FrameLog::create(&dir, &plain_run(48)).expect("the log is made");
    log.start_trace(Instant::now());
    let mut sequence = 0;
    let mut at = 0;
    let mut frame = |log: &mut FrameLog, gap: u64| {
        sequence += 1;
        at += gap;
        draw(log, sequence, at, gap);
    };
    for _ in 0..100 {
        frame(&mut log, 8);
    }
    for _ in 0..3 {
        frame(&mut log, 20);
    }
    // Ten seconds on, another burst, inside the cool-down.
    for _ in 0..1200 {
        frame(&mut log, 8);
    }
    for _ in 0..3 {
        frame(&mut log, 20);
    }
    log.timing(&Timing::Change(PacingChange {
        at_ns: (at + 1) * MS,
        previous_hz: 120.0,
        new_hz: 60.0,
        previous_tier: GraphicsTier::Baseline,
        new_tier: GraphicsTier::Baseline,
        adaptive: None,
    }));
    let path = log.path().to_owned();
    log.finish().expect("the log is finished");
    let read = frame_log::read(&path).expect("the log reads back");
    assert_eq!(read.late.len(), 6);
    assert!(read.late.iter().all(|late| late.frame.is_some()));
    let reasons: Vec<Reason> = read.timing_windows.iter().map(|w| w.reason).collect();
    assert_eq!(reasons, [Reason::Late, Reason::Step, Reason::Exit]);
    // A `late` or `step` window holds the five seconds before it.
    for window in &read.timing_windows[..2] {
        let first = window.events.first().expect("events").at_ns();
        assert!(
            window.end_ns - first <= 5_000 * MS,
            "{}",
            window.end_ns - first
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
