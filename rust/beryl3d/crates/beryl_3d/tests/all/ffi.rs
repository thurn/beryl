//! The C functions as a host calls them. `tests/c/header.c` calls every function of
//! `include/beryl3d.h` with the header's own types and constants, and the frame log the app
//! wrote shows which of its inputs arrived: each valid one, and neither invalid one. An app
//! started with no recording plays a live game, whose first view wakes the host.

use std::ffi::{CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use beryl_3d::ffi::{BerylConfig, beryl3d_close, beryl3d_new};
use beryl_3d::fixture;
use beryl_3d::frame_log::{self, InputLine};

#[allow(unsafe_code)]
unsafe extern "C" {
    /// Defined in `tests/c/header.c`: 0 when every call returned what the header says.
    fn beryl3d_header_test(data_dir: *const c_char) -> i32;
}

/// A folder of this test's own, empty.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl3d-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the folder is made");
    dir
}

#[test]
#[allow(unsafe_code)]
fn the_header_test_calls_every_function_and_its_valid_inputs_arrive() {
    let dir = folder("ffi-header");
    fixture::write("verbs-zones", &dir.join("verbs-zones.json")).expect("the recording writes");
    let path = CString::new(dir.to_string_lossy().as_bytes()).expect("no NUL in the path");
    // SAFETY: the C program takes a NUL-terminated path that lives for the call.
    let failed = unsafe { beryl3d_header_test(path.as_ptr()) };
    assert_eq!(failed, 0, "check {failed} of tests/c/header.c failed");

    let logs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the folder reads")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    let [log] = logs.as_slice() else {
        panic!(
            "expected one frame log in {}, found {logs:?}",
            dir.display()
        );
    };
    let log = frame_log::read(log).expect("the frame log reads");
    assert!(
        (log.run.display_hz - 120.0).abs() < 1e-3,
        "the log counts frames against the display rate the host gave, not {}",
        log.run.display_hz
    );
    let pointers: Vec<(u8, &str)> = log
        .inputs
        .iter()
        .filter_map(|line| match line {
            InputLine::Pointer { id, phase, .. } => Some((*id, phase.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(pointers, [(1, "Down"), (1, "Move"), (1, "Up")]);
    let keys: Vec<(&str, &str)> = log
        .inputs
        .iter()
        .filter_map(|line| match line {
            InputLine::Key { code, phase, .. } => Some((code.as_str(), phase.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(keys, [("Back", "Down"), ("Back", "Up")]);
    let others: Vec<&InputLine> = log
        .inputs
        .iter()
        .filter(|line| !matches!(line, InputLine::Pointer { .. } | InputLine::Key { .. }))
        .collect();
    assert_eq!(
        others,
        [
            &InputLine::Surface {
                width: 800.0,
                height: 600.0,
                scale: 1.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
            &InputLine::Text { length: 5 },
            &InputLine::Lifecycle {
                event: "LowMemory".to_owned()
            },
        ]
    );
    assert!(log.summary.is_some(), "closing the app finishes its log");
}

/// The wake callback the live test gives: it sends on the channel its context points to.
#[allow(unsafe_code)]
unsafe extern "C" fn woken(context: *mut c_void) {
    // SAFETY: the context is the test's sender, alive until the app is closed.
    let sender = unsafe { &*context.cast::<Sender<()>>() };
    let _ = sender.send(());
}

#[test]
#[allow(unsafe_code)]
fn with_no_recording_the_app_plays_a_live_game_whose_views_wake_the_host() {
    let dir = folder("ffi-live");
    let path = CString::new(dir.to_string_lossy().as_bytes()).expect("no NUL in the path");
    let (sender, woke) = channel::<()>();
    let config = BerylConfig {
        fixture: std::ptr::null(),
        data_dir: path.as_ptr(),
        stand_in: std::ptr::null(),
        display_hz: 120.0,
        wake: Some(woken),
        context: std::ptr::from_ref(&sender).cast_mut().cast(),
    };
    // SAFETY: the config's strings live for the call, and the callback's sender outlives the
    // app, which is closed below.
    let app = unsafe { beryl3d_new(&raw const config) };
    assert!(!app.is_null(), "the app starts with no recording");
    // Only a hang reaches this limit: the session's first view comes within a second.
    let woken = woke.recv_timeout(Duration::from_secs(120));
    // SAFETY: the app came from `beryl3d_new` and is not used again.
    unsafe { beryl3d_close(app) };
    assert!(woken.is_ok(), "no view of the live game woke the host");
}
