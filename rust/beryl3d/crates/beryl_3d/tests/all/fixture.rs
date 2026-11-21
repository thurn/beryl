//! A recording written for a phone (`--write-fixture`) reads back from its file and replays
//! through the client to its end, which is when a phone's app finishes its frame log. A missing
//! file is an error that names it.

use std::path::PathBuf;

use app_client::Input;
use app_client::{Client, Config};
use beryl_3d::fixture::{self, Fixture};
use kit_ui::input::{Insets, Size};

/// A sixtieth of a second, in nanoseconds: the ticks of a 60 Hz display.
const TICK: u64 = 1_000_000_000 / 60;
/// How long the replay may take on its own clock before the test fails: `verbs-zones` ends
/// in about 21 seconds.
const LIMIT: u64 = 120 * 1_000_000_000;

/// A folder of this test's own.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl3d-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the folder is made");
    dir
}

#[test]
fn a_written_recording_reads_back_and_replays_to_its_end() {
    let dir = folder("fixture");
    fixture::write("verbs-zones", &dir.join("verbs-zones.json")).expect("the recording writes");
    let mut recording = Fixture::read(&dir, "verbs-zones").expect("the recording reads back");
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: Size {
            width: 852.0,
            height: 393.0,
        },
        scale: 3.0,
        safe_area: Insets::default(),
    });
    let mut clock = 0;
    while !recording.ended(&client) {
        assert!(clock < LIMIT, "the replay had not ended after {LIMIT} ns");
        for update in recording.due(clock) {
            client.handle(Input::Update(Box::new(update)));
        }
        client.frame(clock);
        while let Some(command) = client.next_command() {
            let (reply, views) = recording.answer(&command).expect("the recording answers");
            client.handle(Input::Reply(reply));
            for update in views {
                client.handle(Input::Update(Box::new(update)));
            }
        }
        recording.settled(&client);
        clock += TICK;
    }
    assert!(
        clock > 0,
        "a replay that ends before its first frame showed nothing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_recording_is_an_error_that_names_its_file() {
    let dir = folder("missing");
    let Err(error) = Fixture::read(&dir, "no-such-recording") else {
        panic!("a missing file read as a recording");
    };
    assert!(error.contains("no-such-recording.json"), "{error}");
    let _ = std::fs::remove_dir_all(dir);
}
