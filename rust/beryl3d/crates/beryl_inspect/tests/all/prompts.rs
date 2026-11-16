//! The prompt shapes, played from their recordings: what the flat layer shows for each, the
//! marks the client begins and ends itself while the player answers, and the clicks it sends.

use std::sync::Arc;

use beryl_inspect::pages::{self, Recorded};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Side, Status};

use beryl_protocol::script::{Mark, Subject};
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Duration, Moment, Page, Recording};
use beryl_scene::input::Size;
use beryl_scene::light::{MarkSpan, Span};

/// Long enough for the marks a prompt begins to have come up, or those it ends to have gone.
const SETTLE_MS: u64 = 500;

/// The recording `name` from the viewer's seat, as `hold` leaves it.
fn at(name: &str, view: usize, after: u64) -> Page {
    let mut page = Page::fixture(name, Side::You, Layout::Desktop);
    hold(&mut page, view, after);
    page
}

/// `page` `after` milliseconds past `prompt:<view>`, with the recorded answer to that prompt
/// held back, so that the test answers it.
fn hold(page: &mut Page, view: usize, after: u64) {
    page.set_reply_delay(view, Duration::from_millis(60_000));
    let moment = format!("prompt:{view}");
    page.seek(&moment.parse::<Moment>().unwrap()).unwrap();
    page.step(Duration::from_millis(after));
}

/// The slots of the cards that wear `mark` and are not going.
fn marked(page: &Page, mark: Mark) -> Vec<u16> {
    let mut slots: Vec<u16> = (page.lit().open().iter())
        .filter(|open| open.ends.is_none())
        .filter_map(|open| match open.span {
            Span::Mark(MarkSpan {
                mark: worn,
                on: Subject::Object { slot },
            }) if worn == mark => Some(slot.0),
            _ => None,
        })
        .collect();
    slots.sort_unstable();
    slots
}

/// The prompt bar's words in `page`'s tree.
fn asks(page: &mut Page) -> String {
    let tree = page.tree();
    let line = tree
        .lines()
        .skip_while(|line| !line.contains("region \"prompt\""))
        .nth(1)
        .unwrap_or_else(|| panic!("no prompt bar:\n{tree}"));
    line.trim().to_owned()
}

#[test]
fn a_payment_says_what_is_left_to_pay_unless_a_mana_payment_has_paid_nothing() {
    // The title of a mana payment names the whole cost, so nothing more is said before a pick.
    // The element's id (`#<n>`) counts the elements made before it, so it changes whenever a
    // screen gains one, and is left out.
    let words = asks(&mut at("pay-default", 2, SETTLE_MS));
    assert!(
        words.starts_with("text \"Pay {1}{W}.\" #") && words.ends_with(" 1241,749 102x48"),
        "{words}"
    );
    // A waterbend cost keeps its amount beside the title, as mana symbols.
    let mut page = at("shape-pay", 10, SETTLE_MS);
    let words = asks(&mut page);
    assert!(
        words.starts_with("text \"Tap permanents to Waterbend ({3})\""),
        "{words}"
    );
    // And a crew cost as its power.
    let words = asks(&mut at("shape-pay", 8, SETTLE_MS));
    assert!(words.contains("power 5 or more. (5)\""), "{words}");
}

#[test]
fn hovering_auto_pay_marks_what_it_would_tap_and_moving_off_takes_the_marks_back() {
    let mut page = at("pay-default", 2, SETTLE_MS);
    // Before the pointer is on Auto Pay, what it would tap is outlined as anything else that
    // may be picked.
    assert_eq!(marked(&page, Mark::Eligible), [2, 3, 4]);
    assert_eq!(marked(&page, Mark::Suggested), [] as [u16; 0]);
    page.hover("button \"Auto Pay\"");
    page.step(Duration::from_millis(SETTLE_MS));
    assert_eq!(marked(&page, Mark::Suggested), [2, 4]);
    assert_eq!(marked(&page, Mark::Eligible), [] as [u16; 0]);
    page.move_to(glam::Vec2::new(800.0, 450.0));
    page.step(Duration::from_millis(SETTLE_MS));
    assert_eq!(marked(&page, Mark::Eligible), [2, 3, 4]);
    assert_eq!(marked(&page, Mark::Suggested), [] as [u16; 0]);
    assert!(
        page.lit().open().iter().all(|open| open.ends.is_none()),
        "the marks taken back are gone: {:?}",
        page.lit().open()
    );
}

/// `shape-pay` at its waterbend payment (`prompt:10`), with only that payment's clicks among
/// the recorded answers: the test makes those clicks, and none of the earlier prompts'.
fn waterbend() -> Page {
    const VIEW: usize = 10;
    let Recording(messages) = pages::recording("shape-pay", Side::You).unwrap();
    let views = Recording(messages.clone()).views().unwrap();
    let Status::Deciding { decision } = &views[VIEW - 1].view.status else {
        panic!("view {VIEW} asks for a decision");
    };
    let decision = serde_json::to_value(decision.id).unwrap();
    let messages = messages
        .into_iter()
        .filter(|message| match message {
            FixtureMessage::Response {
                command,
                request: Some(request),
                ..
            } if command == "click" => request["decision"] == decision,
            _ => true,
        })
        .collect();
    let mut page = Page::new(
        Arc::new(Recording(messages)),
        Side::You,
        Layout::Desktop,
        Size {
            width: 1600.0,
            height: 900.0,
        },
    )
    .unwrap();
    hold(&mut page, VIEW, SETTLE_MS);
    page
}

#[test]
fn clicking_an_eligible_permanent_picks_it_to_pay() {
    let mut page = waterbend();
    assert!(marked(&page, Mark::Eligible).contains(&11));
    // The recording answers only the click it holds, on Trinket Mage: any other click fails
    // the test.
    page.press("card \"Trinket Mage\" #11");
    page.step(Duration::from_millis(SETTLE_MS));
    assert_eq!(marked(&page, Mark::Paying), [11]);
    assert!(!marked(&page, Mark::Eligible).contains(&11));
    let words = asks(&mut page);
    assert!(words.contains("({2})"), "{words}");
}

#[test]
fn a_payment_answered_ends_its_marks() {
    // Auto Pay answers the mana payment, and the next prompt marks none of its lands.
    let page = at("pay-default", 3, SETTLE_MS);
    assert!(
        (page.lit().open().iter()).all(|open| !matches!(open.span, Span::Mark(_))),
        "{:?}",
        page.lit().open()
    );
}
