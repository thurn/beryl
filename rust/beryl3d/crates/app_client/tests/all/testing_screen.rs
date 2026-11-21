//! The testing screen, driven through the `Page` test driver: the key T on the start screen,
//! the filter field, the seat toggle and the replay a press on a recording asks for.

use app_client::command::Request;
use app_client::driver::{OPEN, Page, RECORDINGS, Recording};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use kit_ui::input::Size;

/// The names a tools build lists, as the host gives them.
const NAMES: [&str; 4] = [
    "absorb/countered",
    "counterspell/default",
    "dissipate/default",
    "verbs-zones",
];

/// The start screen over the sample table, of a client given `names` as its recordings, as a
/// tools build gives them, or none.
fn start(names: Option<&[&str]>) -> Page {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let mut messages: Vec<FixtureMessage> = names
        .map(|names| FixtureMessage::Event {
            event: RECORDINGS.to_owned(),
            payload: names.to_vec().into(),
        })
        .into_iter()
        .collect();
    messages.push(FixtureMessage::Event {
        event: OPEN.to_owned(),
        payload: "start".into(),
    });
    messages.push(FixtureMessage::game_update(&view.update()));
    let recording = Recording(messages);
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = Page::new(
        crate::test_data::data(),
        recording.into(),
        Side::You,
        crate::REGULAR,
        size,
    )
    .unwrap();
    page.still();
    page
}

/// The start screen, then the testing screen its key T opens.
fn testing() -> Page {
    let mut page = start(Some(&NAMES));
    page.chord("T");
    page.still();
    page
}

/// Every request the client has made since the last call.
fn requests(page: &mut Page) -> Vec<Request> {
    std::iter::from_fn(|| page.next_request()).collect()
}

/// The names of the recordings the testing screen's list shows, in order.
fn listed(page: &mut Page) -> Vec<String> {
    let tree = page.tree();
    NAMES
        .iter()
        .filter(|name| tree.contains(&format!("button \"{name}\"")))
        .map(|name| (*name).to_owned())
        .collect()
}

#[test]
fn the_key_t_opens_the_testing_screen_with_every_recording() {
    let mut page = testing();
    assert!(
        page.tree().contains("region \"testing\""),
        "{}",
        page.tree()
    );
    assert_eq!(listed(&mut page), NAMES);
}

#[test]
fn typing_counter_narrows_the_list_to_the_names_that_contain_it() {
    let mut page = testing();
    page.type_text("field \"filter\"", "counter");
    page.still();
    assert_eq!(
        listed(&mut page),
        ["absorb/countered", "counterspell/default"]
    );
}

#[test]
fn a_press_on_a_recording_replays_it_from_the_seat_the_toggle_shows() {
    let mut page = testing();
    requests(&mut page);
    page.press("toggle \"Them\"");
    page.still();
    page.press("button \"dissipate/default\"");
    let replays: Vec<Request> = requests(&mut page)
        .into_iter()
        .filter(|request| matches!(request, Request::Replay { .. }))
        .collect();
    assert_eq!(
        replays,
        [Request::Replay {
            name: "dissipate/default".to_owned(),
            seat: Side::Opponent,
        }]
    );
}

#[test]
fn a_press_with_the_toggle_off_replays_from_the_viewers_seat() {
    let mut page = testing();
    requests(&mut page);
    page.press("button \"verbs-zones\"");
    assert!(requests(&mut page).contains(&Request::Replay {
        name: "verbs-zones".to_owned(),
        seat: Side::You,
    }));
}

#[test]
fn with_no_recordings_the_key_t_does_not_leave_the_start_screen() {
    let mut page = start(None);
    page.chord("T");
    page.still();
    let tree = page.tree();
    assert!(tree.contains("region \"start\""), "{tree}");
    assert!(!tree.contains("region \"testing\""), "{tree}");
}
