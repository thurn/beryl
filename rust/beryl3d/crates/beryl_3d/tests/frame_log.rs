//! The frame log's writer and reader agree: what a run writes reads back as the same run, the
//! same late frames and the same summary.

use std::path::PathBuf;

use beryl_3d::frame_log::{self, FrameLog, Run};
use beryl_host::rate::Step;
use beryl_host::{Shown, Ticked};

const MS: u64 = 1_000_000;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl3d-{name}-{}", std::process::id()));
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
    assert_eq!(read.summary.as_ref(), Some(&summary));
    assert_eq!(summary.frames, 7);
    assert_eq!(summary.skipped, 1);
    assert_eq!(
        (summary.late, summary.held_to_60, summary.missed_60),
        (3, 2, 1)
    );
    assert_eq!(summary.after_pause, 1);
    assert_eq!(summary.unbuilt, ["tap"]);
    let gaps: Vec<f64> = read.late.iter().map(|late| late.since_last_ms).collect();
    assert_eq!(gaps, [17.0, 40.0, 17.0]);
    assert_eq!(read.late[0].verbs, ["fly", "set"]);
    assert!(read.late[0].searching && read.late[0].held_to_60);
    assert!(!read.late[1].held_to_60);
    let printed = summary.print(&read.run);
    assert!(
        printed.contains("drive hover-hand, load threads 16:utility, slow GPU 5 ms"),
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
