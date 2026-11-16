//! The prompt shape `Order` on its recording (docs/beryl-3d.md, "The flat layer"): waiting
//! triggers held up in a row over the shade, the first to resolve at the right, a click on one
//! putting it first and one carried along the row dropped where it lands. Each test plays the
//! recording to the prompt, acts as the player did, and lets the driver answer each click with
//! the session's recorded reply, which fails the test for any click the player did not make.

use std::str::FromStr;

use beryl_inspect::pages::page;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::Layout;
use beryl_scene::driver::{Duration, Moment, Page};
use beryl_scene::input::{Rect, Size};
use glam::Vec2;

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// `shape-order`'s fourth prompt, at its fourth view, orders two triggers of one Currency
/// Converter, which the session lists as they will resolve, the candidates 0 then 1. The
/// recording holds one click on it: candidate 1 dropped at the front (`Click::Drop` on ability
/// 1, pile 0, at 0), whether by a click or by carrying it to the right end.
const ORDER: usize = 4;

/// `shape-order` played from the viewer's seat to the order prompt, when the table has come to
/// rest under it. The recorded answer to that prompt is held back, so that the test answers it.
fn at_prompt() -> Page {
    let mut page =
        page("shape-order", Side::You, Layout::Desktop, SIZE).unwrap_or_else(|why| panic!("{why}"));
    page.set_reply_delay(ORDER, Duration::from_millis(60_000));
    let at = format!("prompt:{ORDER}");
    let moment = Moment::from_str(&at).unwrap_or_else(|why| panic!("{at}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.step(Duration::from_millis(400));
    page
}

/// The boxes of the row's items, the first to resolve first. Both triggers share their source,
/// and each is named for its place in the order.
fn items(page: &mut Page) -> Vec<Rect> {
    ["1. Currency Converter", "2. Currency Converter"]
        .map(|name| page.find(&format!("ability \"{name}\"")).rect())
        .to_vec()
}

/// Presses Done, which the driver answers only on the prompt's state after the recorded drop
/// (its revision 1), and so only once the test's drop reached the session, then waits for the
/// prompt to close.
fn done_after_the_drop(page: &mut Page) {
    page.press("button \"Done\"");
    page.step(Duration::from_millis(600));
    assert!(
        page.try_find("button \"Done\"").is_err(),
        "Done answered the prompt"
    );
}

/// The middle of `rect`.
fn middle(rect: Rect) -> Vec2 {
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

#[test]
fn an_orders_triggers_stand_in_a_row_the_first_to_resolve_at_the_right() {
    let mut page = at_prompt();
    let row = items(&mut page);
    assert_eq!(
        row.len(),
        2,
        "both waiting triggers are in the row: {row:?}"
    );
    assert!(
        row[0].x > row[1].x + row[1].width,
        "the first to resolve lies right of the second: {row:?}"
    );
    assert!(
        (row[0].y - row[1].y).abs() < 0.5,
        "the items are level: {row:?}"
    );
    let last = page.find("text \"Last\"").rect();
    let first = page.find("text \"First\"").rect();
    assert!(
        last.x < first.x && last.y > row[0].y + row[0].height,
        "the session's words for the ends lie under the row, the last at the left: {last:?} \
         and {first:?}"
    );
}

#[test]
fn a_click_on_a_trigger_puts_it_first() {
    let mut page = at_prompt();
    let row = items(&mut page);
    // The leftmost, candidate 1: the driver answers only the recorded drop of it at 0.
    page.drag_between(middle(row[1]), middle(row[1]));
    page.step(Duration::from_millis(600));
    assert_eq!(items(&mut page).len(), 2, "the reply's order is drawn");
    done_after_the_drop(&mut page);
}

#[test]
fn a_trigger_carried_to_the_right_end_is_dropped_at_the_front() {
    let mut page = at_prompt();
    let row = items(&mut page);
    let right_end = Vec2::new(
        row[0].x + row[0].width + 20.0,
        row[0].y + row[0].height / 2.0,
    );
    page.drag_between(middle(row[1]), right_end);
    page.step(Duration::from_millis(600));
    let after = items(&mut page);
    assert_eq!(
        after.len(),
        2,
        "the carried item is back in the row and nothing else is carried: {after:?}"
    );
    done_after_the_drop(&mut page);
}
