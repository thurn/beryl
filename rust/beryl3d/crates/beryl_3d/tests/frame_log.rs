//! The frame log's writer and reader agree: what a run writes reads back as the same run, the
//! same late frames and the same summary.

use std::path::PathBuf;

use beryl_3d::frame_log::{self, FrameLog, Run};
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
    let _ = std::fs::remove_dir_all(&dir);
}
